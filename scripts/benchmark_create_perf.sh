#!/usr/bin/env bash
# Compare par2rs create against par2cmdline-turbo using instruction counts.
#
# This harness is designed for loaded machines: wall clock is captured only as
# context, while Linux perf hardware counters are the primary comparison signal.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

ITERATIONS="${ITERATIONS:-30}"
WARMUP_RUNS="${WARMUP_RUNS:-3}"
REDUNDANCY="${REDUNDANCY:-10}"
RECOVERY_FILES="${RECOVERY_FILES:-1}"
THREADS="${THREADS:-1}"
PAR2CMD_BIN="${PAR2CMD_BIN:-par2}"
PAR2RS_BIN="${PAR2RS_BIN:-$PROJECT_ROOT/target/release/par2}"
RESULTS_ROOT="${RESULTS_ROOT:-$PROJECT_ROOT/target/perf-results/create}"
KEEP_WORK="${KEEP_WORK:-0}"
RUN_FLAMEGRAPH="${RUN_FLAMEGRAPH:-1}"
PROFILE_FREQUENCY="${PROFILE_FREQUENCY:-997}"
PERF_EVENTS="${PERF_EVENTS:-instructions,cycles,branches,branch-misses,cache-references,cache-misses,task-clock,context-switches,cpu-migrations,page-faults}"
VERIFY_OUTPUTS="${VERIFY_OUTPUTS:-1}"
VERIFY_REPAIR="${VERIFY_REPAIR:-smoke}"

# label:file_count:file_size_mib:block_size_bytes
DEFAULT_CASES="single_16m:1:16:262144,single_128m:1:128:1048576,multi_128m:32:4:262144"
CASES="${CASES:-$DEFAULT_CASES}"

usage() {
    cat <<EOF
Usage: $(basename "$0")

Environment:
  ITERATIONS=N       measured runs per tool/case (default: $ITERATIONS)
  WARMUP_RUNS=N      unmeasured warmups per tool/case (default: $WARMUP_RUNS)
  CASES=SPEC         comma-separated label:file_count:file_size_mib:block_size
                     default: $DEFAULT_CASES
  THREADS=N          pass -t N to both tools; 0 omits -t (default: $THREADS)
  REDUNDANCY=N       create redundancy percentage (default: $REDUNDANCY)
  RECOVERY_FILES=N   recovery file count via -n (default: $RECOVERY_FILES)
  PAR2CMD_BIN=PATH   par2cmdline-turbo binary (default: $PAR2CMD_BIN)
  VERIFY_OUTPUTS=0   skip cross-tool verification after each create
  VERIFY_REPAIR=MODE run destructive cross-tool repair validation: smoke, 1, or 0
                     default: $VERIFY_REPAIR
  RUN_FLAMEGRAPH=0   skip par2rs perf/inferno flamegraph generation
  PROFILE_FREQUENCY=N perf sampling frequency for flamegraph (default: $PROFILE_FREQUENCY)
  KEEP_WORK=1        keep generated benchmark work directory

Example:
  nix develop --command env ITERATIONS=40 THREADS=16 \\
    CASES='single_256m:1:256:1048576,multi_1g:64:16:1048576' \\
    scripts/benchmark_create_perf.sh
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

need_tool() {
    local tool="$1"
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: required tool not found: $tool" >&2
        exit 1
    fi
}

need_tool perf
need_tool python3
need_tool dd
need_tool cmp
need_tool "$PAR2CMD_BIN"

if ! perf stat -x, -e instructions -- true >/dev/null 2>&1; then
    echo "error: perf cannot read the instructions counter." >&2
    echo "Check kernel.perf_event_paranoid or run in an environment with perf permissions." >&2
    exit 1
fi

mkdir -p "$RESULTS_ROOT"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
RUN_ROOT="$RESULTS_ROOT/run-$TIMESTAMP"
WORK_ROOT="$RUN_ROOT/work"
RAW_CSV="$RUN_ROOT/raw.csv"
SUMMARY_MD="$RUN_ROOT/summary.md"
SMOKE_CSV="$RUN_ROOT/smoke.csv"
SMOKE_SUMMARY_MD="$RUN_ROOT/smoke-summary.md"
PRESERVE_WORK=0
mkdir -p "$WORK_ROOT"

cleanup() {
    if [[ "$KEEP_WORK" != "1" && "$PRESERVE_WORK" != "1" ]]; then
        rm -rf "$WORK_ROOT"
    fi
}
trap cleanup EXIT

echo "=== par2rs create perf benchmark ==="
echo "results: $RUN_ROOT"
echo "iterations: $ITERATIONS measured, $WARMUP_RUNS warmup"
echo "cases: $CASES"
echo "threads: $THREADS"
echo

echo "Building par2rs release binary..."
(cd "$PROJECT_ROOT" && cargo build --release --bin par2 --quiet)

write_csv_header() {
    local csv_file="$1"
    printf 'case,tool,selected_method,run,status,validation_status,instructions,cycles,branches,branch_misses,cache_references,cache_misses,task_clock_msec,context_switches,cpu_migrations,page_faults,wall_seconds\n' > "$csv_file"
}

write_csv_header "$RAW_CSV"
write_csv_header "$SMOKE_CSV"

split_cases() {
    tr ',' '\n' <<< "$CASES"
}

make_corpus() {
    local case_dir="$1"
    local file_count="$2"
    local file_size_mib="$3"

    mkdir -p "$case_dir/corpus"
    local i
    for i in $(seq 1 "$file_count"); do
        local file
        file="$case_dir/corpus/file_$(printf '%04d' "$i").bin"
        if [[ ! -f "$file" ]]; then
            python3 - "$file" "$i" "$file_size_mib" <<'PY'
import sys

path = sys.argv[1]
file_index = int(sys.argv[2])
size_mib = int(sys.argv[3])
chunk_size = 1024 * 1024

with open(path, "wb") as f:
    for chunk_index in range(size_mib):
        value = (file_index * 131 + chunk_index * 17) & 0xFF
        chunk = bytearray(bytes([value]) * chunk_size)
        chunk[:16] = file_index.to_bytes(8, "little") + chunk_index.to_bytes(8, "little")
        f.write(chunk)
PY
        fi
    done
}

link_corpus() {
    local corpus_dir="$1"
    local run_dir="$2"

    mkdir -p "$run_dir"
    local file base
    for file in "$corpus_dir"/*.bin; do
        base="$(basename "$file")"
        ln "$file" "$run_dir/$base" 2>/dev/null || cp -p "$file" "$run_dir/$base"
    done
}

perf_value() {
    local perf_file="$1"
    local event="$2"
    awk -F, -v event="$event" '
        {
            parsed_event = $3
            sub(/:.*/, "", parsed_event)
        }
        parsed_event == event {
            gsub(/[[:space:]]/, "", $1)
            if ($1 == "" || $1 ~ /^<not/) {
                print ""
            } else {
                print $1
            }
            exit
        }
    ' "$perf_file"
}

run_create() {
    local case_label="$1"
    local tool="$2"
    local run_number="$3"
    local block_size="$4"
    local corpus_dir="$5"
    local measured="$6"
    local output_csv="${7:-$RAW_CSV}"

    local run_dir="$WORK_ROOT/$case_label/$tool-$run_number"
    local perf_file="$RUN_ROOT/perf-$case_label-$tool-$run_number.csv"
    rm -rf "$run_dir"
    link_corpus "$corpus_dir" "$run_dir"

    local -a cmd
    local selected_method="$tool"
    local -a env_prefix=()
    if [[ "$tool" == par2rs-* ]]; then
        case "$tool" in
            par2rs-pshufb)
                selected_method="pshufb"
                env_prefix=(env PAR2RS_CREATE_GF16=pshufb)
                ;;
            par2rs-xor-jit)
                selected_method="xor-jit"
                env_prefix=(env PAR2RS_CREATE_GF16=xor-jit)
                ;;
            *)
                echo "error: unknown par2rs variant: $tool" >&2
                return 2
                ;;
        esac
        cmd=("$PAR2RS_BIN" c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    else
        selected_method="auto"
        cmd=("$PAR2CMD_BIN" c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    fi
    if [[ "$THREADS" != "0" ]]; then
        cmd+=("-t$THREADS")
    fi

    local start_ns end_ns status validation_status wall_seconds
    start_ns="$(date +%s%N)"
    set +e
    (
        cd "$run_dir"
        sources=(./*.bin)
        perf stat -x, -o "$perf_file" -e "$PERF_EVENTS" -- "${env_prefix[@]}" "${cmd[@]}" out.par2 "${sources[@]}" >/dev/null
    )
    status="$?"
    set -e
    end_ns="$(date +%s%N)"
    wall_seconds="$(awk -v s="$start_ns" -v e="$end_ns" 'BEGIN { printf "%.6f", (e - s) / 1000000000 }')"
    validation_status=0

    if [[ "$status" == "0" && "$VERIFY_OUTPUTS" == "1" ]]; then
        local verifier_stdout verifier_stderr
        verifier_stdout="$RUN_ROOT/verify-$case_label-$tool-$run_number.stdout"
        verifier_stderr="$RUN_ROOT/verify-$case_label-$tool-$run_number.stderr"
        set +e
        if [[ "$tool" == par2rs-* ]]; then
            (cd "$run_dir" && "$PAR2CMD_BIN" v -q -q out.par2 >"$verifier_stdout" 2>"$verifier_stderr")
        else
            (cd "$run_dir" && "$PAR2RS_BIN" verify -q -q out.par2 >"$verifier_stdout" 2>"$verifier_stderr")
        fi
        validation_status="$?"
        set -e
        if [[ "$validation_status" != "0" ]]; then
            {
                echo
                echo "quiet validation failed with status $validation_status; rerunning verbosely"
            } >> "$verifier_stderr"
            set +e
            if [[ "$tool" == par2rs-* ]]; then
                (cd "$run_dir" && "$PAR2CMD_BIN" v out.par2 >>"$verifier_stdout" 2>>"$verifier_stderr")
            else
                (cd "$run_dir" && "$PAR2RS_BIN" verify out.par2 >>"$verifier_stdout" 2>>"$verifier_stderr")
            fi
            set -e
            echo "error: cross-tool verification failed for $case_label/$tool run $run_number" >&2
            echo "stdout: $verifier_stdout" >&2
            echo "stderr: $verifier_stderr" >&2
            echo "work dir: $run_dir" >&2
        else
            rm -f "$verifier_stdout" "$verifier_stderr"
        fi

        if [[ "$validation_status" == "0" ]] && should_repair_validate "$run_number"; then
            local repair_stdout repair_stderr
            repair_stdout="$RUN_ROOT/repair-$case_label-$tool-$run_number.stdout"
            repair_stderr="$RUN_ROOT/repair-$case_label-$tool-$run_number.stderr"
            set +e
            validate_repair_output "$tool" "$run_dir" "$corpus_dir" "$block_size" "$repair_stdout" "$repair_stderr"
            validation_status="$?"
            set -e
            if [[ "$validation_status" != "0" ]]; then
                echo "error: cross-tool repair validation failed for $case_label/$tool run $run_number" >&2
                echo "stdout: $repair_stdout" >&2
                echo "stderr: $repair_stderr" >&2
                echo "work dir: $run_dir" >&2
            else
                rm -f "$repair_stdout" "$repair_stderr"
            fi
        fi
    fi

    if [[ "$measured" == "1" ]]; then
        local instructions cycles branches branch_misses cache_references cache_misses
        local task_clock context_switches cpu_migrations page_faults
        instructions="$(perf_value "$perf_file" instructions)"
        cycles="$(perf_value "$perf_file" cycles)"
        branches="$(perf_value "$perf_file" branches)"
        branch_misses="$(perf_value "$perf_file" branch-misses)"
        cache_references="$(perf_value "$perf_file" cache-references)"
        cache_misses="$(perf_value "$perf_file" cache-misses)"
        task_clock="$(perf_value "$perf_file" task-clock)"
        context_switches="$(perf_value "$perf_file" context-switches)"
        cpu_migrations="$(perf_value "$perf_file" cpu-migrations)"
        page_faults="$(perf_value "$perf_file" page-faults)"
        printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
            "$case_label" "$tool" "$selected_method" "$run_number" "$status" "$validation_status" \
            "$instructions" "$cycles" "$branches" "$branch_misses" \
            "$cache_references" "$cache_misses" "$task_clock" \
            "$context_switches" "$cpu_migrations" "$page_faults" "$wall_seconds" >> "$output_csv"
    fi

    if [[ "$status" == "0" && "$validation_status" == "0" ]]; then
        rm -rf "$run_dir"
    elif [[ "$KEEP_WORK" != "1" ]]; then
        PRESERVE_WORK=1
        echo "preserving failed run directory: $run_dir" >&2
    fi
    if [[ "$status" != "0" ]]; then
        return "$status"
    fi
    return "$validation_status"
}

should_repair_validate() {
    local run_number="$1"
    case "$VERIFY_REPAIR" in
        1|true|yes)
            return 0
            ;;
        smoke)
            [[ "$run_number" == "smoke" ]]
            return
            ;;
        *)
            return 1
            ;;
    esac
}

validate_repair_output() {
    local tool="$1"
    local run_dir="$2"
    local corpus_dir="$3"
    local block_size="$4"
    local repair_stdout="$5"
    local repair_stderr="$6"

    local target base
    target="$(find "$run_dir" -maxdepth 1 -type f -name '*.bin' | sort | head -n 1)"
    if [[ -z "$target" ]]; then
        echo "no source file found to damage" > "$repair_stderr"
        return 97
    fi
    base="$(basename "$target")"

    cp -p "$target" "$target.detached"
    mv "$target.detached" "$target"
    python3 - "$target" "$block_size" <<'PY'
import sys

path = sys.argv[1]
block_size = int(sys.argv[2])

with open(path, "r+b") as f:
    data = f.read(block_size)
    if not data:
        sys.exit(2)
    f.seek(0)
    f.write(bytes(b ^ 0xFF for b in data))
PY

    if [[ "$tool" == par2rs-* ]]; then
        (cd "$run_dir" && "$PAR2CMD_BIN" r -q -q out.par2 >"$repair_stdout" 2>"$repair_stderr")
    else
        (cd "$run_dir" && "$PAR2RS_BIN" repair -q -q out.par2 >"$repair_stdout" 2>"$repair_stderr")
    fi || return "$?"

    if ! cmp -s "$corpus_dir/$base" "$run_dir/$base"; then
        echo "repaired file differs from original corpus file: $base" >> "$repair_stderr"
        return 98
    fi
}

summarize_results() {
    local raw_csv="${1:-$RAW_CSV}"
    local summary_md="${2:-$SUMMARY_MD}"
    local iterations="${3:-$ITERATIONS}"
    local warmups="${4:-$WARMUP_RUNS}"
    local title="${5:-PAR2 Create Perf Benchmark}"

    python3 - "$raw_csv" "$summary_md" "$iterations" "$warmups" "$THREADS" "$REDUNDANCY" "$RECOVERY_FILES" "$title" <<'PY'
import csv
import math
import statistics
import sys
from collections import defaultdict

raw_csv, summary_md, iterations, warmups, threads, redundancy, recovery_files, title = sys.argv[1:]
metrics = [
    ("instructions", "instructions"),
    ("cycles", "cycles"),
    ("branches", "branches"),
    ("branch_misses", "branch misses"),
    ("cache_misses", "cache misses"),
    ("task_clock_msec", "task-clock ms"),
    ("wall_seconds", "wall seconds"),
]

rows = []
with open(raw_csv, newline="") as f:
    for row in csv.DictReader(f):
        rows.append(row)

groups = defaultdict(list)
for row in rows:
    groups[(row["case"], row["tool"])].append(row)

def number(row, key):
    value = row.get(key, "")
    if not value:
        return None
    try:
        return float(value)
    except ValueError:
        return None

def stats(values):
    values = [v for v in values if v is not None and math.isfinite(v)]
    if not values:
        return None
    mean = statistics.fmean(values)
    stdev = statistics.stdev(values) if len(values) > 1 else 0.0
    cv = stdev / mean * 100 if mean else 0.0
    return mean, stdev, cv, min(values), max(values), len(values)

def fmt(value):
    if value is None:
        return "n/a"
    if abs(value) >= 1_000_000:
        return f"{value:,.0f}"
    if abs(value) >= 100:
        return f"{value:,.2f}"
    return f"{value:.4f}"

cases = sorted({row["case"] for row in rows})

with open(summary_md, "w") as out:
    out.write(f"# {title}\n\n")
    out.write(f"- measured runs per tool/case: {iterations}\n")
    out.write(f"- warmup runs per tool/case: {warmups}\n")
    out.write(f"- threads: {threads}\n")
    out.write(f"- redundancy: {redundancy}%\n")
    out.write(f"- recovery files: {recovery_files}\n")
    out.write("- validation: par2rs output verified/repaired by turbo; turbo output verified/repaired by par2rs\n")
    out.write("- primary signal: Linux perf `instructions`\n\n")

    out.write("## Relative Summary\n\n")
    out.write("| case | tool | method | instructions | cycles | wall seconds | effective CPU |\n")
    out.write("|---|---|---|---:|---:|---:|---:|\n")
    for case in cases:
        case_tools = sorted({row["tool"] for row in rows if row["case"] == case})
        for tool in case_tools:
            group = groups[(case, tool)]
            method = group[0].get("selected_method", tool) if group else tool
            instr = stats(number(r, "instructions") for r in group)
            cycles = stats(number(r, "cycles") for r in group)
            wall = stats(number(r, "wall_seconds") for r in group)
            task_clock = stats(number(r, "task_clock_msec") for r in group)
            effective_cpu = task_clock[0] / (wall[0] * 1000) if task_clock and wall and wall[0] else None
            out.write(
                f"| {case} | {tool} | {method} | {fmt(instr[0] if instr else None)} | "
                f"{fmt(cycles[0] if cycles else None)} | {fmt(wall[0] if wall else None)} | {fmt(effective_cpu)} |\n"
            )

    out.write("\n## Detailed Stats\n\n")
    out.write("| case | tool | metric | n | mean | stddev | cv % | min | max |\n")
    out.write("|---|---|---|---:|---:|---:|---:|---:|---:|\n")
    for case in cases:
        for tool in sorted({row["tool"] for row in rows if row["case"] == case}):
            group = groups[(case, tool)]
            failures = [r for r in group if r["status"] != "0"]
            validation_failures = [r for r in group if r.get("validation_status", "0") != "0"]
            if failures:
                out.write(f"| {case} | {tool} | failures | {len(failures)} | status != 0 |  |  |  |  |\n")
            if validation_failures:
                out.write(f"| {case} | {tool} | validation failures | {len(validation_failures)} | validation_status != 0 |  |  |  |  |\n")
            for key, label in metrics:
                s = stats(number(r, key) for r in group)
                if s is None:
                    out.write(f"| {case} | {tool} | {label} | 0 | n/a | n/a | n/a | n/a | n/a |\n")
                    continue
                mean, stdev, cv, min_v, max_v, n = s
                out.write(f"| {case} | {tool} | {label} | {n} | {fmt(mean)} | {fmt(stdev)} | {cv:.2f} | {fmt(min_v)} | {fmt(max_v)} |\n")
PY
}

run_smoke_benchmarks() {
    echo
    echo "Smoke benchmark: one measured, cross-validated run for each configured case/tool"
    while IFS= read -r case_spec; do
        [[ -z "$case_spec" ]] && continue
        IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
        if [[ -z "${label:-}" || -z "${file_count:-}" || -z "${file_size_mib:-}" || -z "${block_size:-}" ]]; then
            echo "error: invalid CASES entry: $case_spec" >&2
            exit 1
        fi

        local case_dir="$WORK_ROOT/$label"
        local corpus_dir="$case_dir/corpus"
        echo "Smoke preparing '$label': ${file_count}x${file_size_mib}MiB, block size ${block_size}"
        make_corpus "$case_dir" "$file_count" "$file_size_mib"

        echo "Smoke measured: $label / par2rs-pshufb"
        run_create "$label" "par2rs-pshufb" "smoke" "$block_size" "$corpus_dir" 1 "$SMOKE_CSV"

        echo "Smoke measured: $label / par2rs-xor-jit"
        run_create "$label" "par2rs-xor-jit" "smoke" "$block_size" "$corpus_dir" 1 "$SMOKE_CSV"

        echo "Smoke measured: $label / turbo-auto"
        run_create "$label" "turbo-auto" "smoke" "$block_size" "$corpus_dir" 1 "$SMOKE_CSV"
    done < <(split_cases)

    summarize_results "$SMOKE_CSV" "$SMOKE_SUMMARY_MD" 1 0 "PAR2 Create Perf Smoke Benchmark"
    echo "smoke raw data: $SMOKE_CSV"
    echo "smoke summary:  $SMOKE_SUMMARY_MD"
    echo
}

summarize_perf_script() {
    python3 -c '
import re
import sys
from collections import Counter

samples = 0
leaf = Counter()
inclusive = Counter()
current = []

def flush():
    global samples, current
    if not current:
        return
    samples += 1
    leaf[current[0]] += 1
    inclusive.update(set(current))
    current = []

def parse_frame(line):
    line = line.strip()
    if not line:
        return None
    parts = line.split(None, 1)
    if len(parts) != 2:
        return None
    rest = parts[1]
    rest = re.sub(r" \([^)]*\)$", "", rest)
    rest = re.sub(r"\+0x[0-9a-fA-F]+$", "", rest)
    return rest or None

for raw in sys.stdin:
    line = raw.rstrip("\n")
    if not line:
        flush()
    elif line.startswith((" ", "\t")):
        frame = parse_frame(line)
        if frame and frame != "[unknown]":
            current.append(frame)
flush()

if samples == 0:
    print("No perf script samples parsed.")
    sys.exit(0)

print(f"Total samples: {samples}")
print()
print("Top self-time functions:")
for name, count in leaf.most_common(40):
    print(f"{count / samples * 100:6.2f}% {count:8d}  {name[:120]}")
print()
print("Top inclusive functions:")
for name, count in inclusive.most_common(40):
    print(f"{count / samples * 100:6.2f}% {count:8d}  {name[:120]}")
'
}

generate_flamegraph() {
    local case_spec="$1"
    IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
    local case_dir="$WORK_ROOT/$label"
    local corpus_dir="$case_dir/corpus"
    local flamegraph_file="$RUN_ROOT/par2rs-create-$label-flamegraph.svg"
    local output_file="$case_dir/flamegraph-out.par2"

    if [[ "$RUN_FLAMEGRAPH" != "1" ]]; then
        return 0
    fi
    if ! command -v inferno-collapse-perf >/dev/null 2>&1 || ! command -v inferno-flamegraph >/dev/null 2>&1; then
        if command -v cargo >/dev/null 2>&1; then
            echo "inferno not found; installing with cargo install inferno..."
            cargo install inferno
        else
            echo "warning: inferno is unavailable; skipping flamegraph" >&2
            return 0
        fi
    fi

    rm -f "$case_dir"/flamegraph-out*.par2
    local -a args=(c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    if [[ "$THREADS" != "0" ]]; then
        args+=("-t$THREADS")
    fi
    args+=("$output_file")
    local file
    for file in "$corpus_dir"/*.bin; do
        args+=("$file")
    done

    echo
    echo "Building par2rs profiling binary with frame pointers..."
    (
        cd "$PROJECT_ROOT"
        RUSTFLAGS="-C target-cpu=native -C force-frame-pointers=yes" \
            cargo build --profile profiling --bin par2 --quiet
    )

    local profiling_bin="$PROJECT_ROOT/target/profiling/par2"
    local perf_data="$RUN_ROOT/par2rs-create-$label.perf.data"
    local perf_summary="$RUN_ROOT/par2rs-create-$label.perf-report.txt"
    local hotspots="$RUN_ROOT/par2rs-create-$label.hotspots.txt"

    echo "Recording par2rs flamegraph for case '$label'..."
    if ! perf record -g --call-graph fp -F "$PROFILE_FREQUENCY" -o "$perf_data" -- "$profiling_bin" "${args[@]}" >/dev/null; then
        echo "warning: flamegraph generation failed; benchmark results are still available" >&2
        return 0
    fi

    echo "Rendering flamegraph SVG..."
    perf script -i "$perf_data" 2>/dev/null | inferno-collapse-perf | inferno-flamegraph > "$flamegraph_file"

    echo "Generating perf report text..."
    perf report -i "$perf_data" --stdio --sort comm,dso,symbol > "$perf_summary" 2>/dev/null || true

    echo "Generating hotspot summary..."
    perf script -i "$perf_data" 2>/dev/null | summarize_perf_script > "$hotspots" || true

    echo "flamegraph: $flamegraph_file"
    echo "perf data:  $perf_data"
    echo "perf text:  $perf_summary"
    echo "hotspots:   $hotspots"
}

last_case=""
run_smoke_benchmarks

while IFS= read -r case_spec; do
    [[ -z "$case_spec" ]] && continue
    IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
    if [[ -z "${label:-}" || -z "${file_count:-}" || -z "${file_size_mib:-}" || -z "${block_size:-}" ]]; then
        echo "error: invalid CASES entry: $case_spec" >&2
        exit 1
    fi

    last_case="$case_spec"
    case_dir="$WORK_ROOT/$label"
    corpus_dir="$case_dir/corpus"
    echo "Preparing case '$label': ${file_count}x${file_size_mib}MiB, block size ${block_size}"
    make_corpus "$case_dir" "$file_count" "$file_size_mib"

    for tool in par2rs-pshufb par2rs-xor-jit turbo-auto; do
        echo "Warmup: $label / $tool"
        for run in $(seq 1 "$WARMUP_RUNS"); do
            run_create "$label" "$tool" "warmup-$run" "$block_size" "$corpus_dir" 0
        done

        echo "Measured: $label / $tool"
        for run in $(seq 1 "$ITERATIONS"); do
            printf '  %s %-6s run %2d/%d\r' "$label" "$tool" "$run" "$ITERATIONS"
            run_create "$label" "$tool" "$run" "$block_size" "$corpus_dir" 1
        done
        printf '\n'
    done
done < <(split_cases)

summarize_results
if [[ -n "$last_case" ]]; then
    generate_flamegraph "$last_case"
fi

echo
echo "raw data: $RAW_CSV"
echo "summary:  $SUMMARY_MD"
if [[ "$KEEP_WORK" == "1" ]]; then
    echo "work dir:  $WORK_ROOT"
fi

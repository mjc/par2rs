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
mkdir -p "$WORK_ROOT"

cleanup() {
    if [[ "$KEEP_WORK" != "1" ]]; then
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

printf 'case,tool,run,status,instructions,cycles,branches,branch_misses,cache_references,cache_misses,task_clock_msec,context_switches,cpu_migrations,page_faults,wall_seconds\n' > "$RAW_CSV"

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
            dd if=/dev/zero of="$file" bs=1M count="$file_size_mib" status=none
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

    local run_dir="$WORK_ROOT/$case_label/$tool-$run_number"
    local perf_file="$RUN_ROOT/perf-$case_label-$tool-$run_number.csv"
    rm -rf "$run_dir"
    link_corpus "$corpus_dir" "$run_dir"

    local -a cmd
    if [[ "$tool" == "par2rs" ]]; then
        cmd=("$PAR2RS_BIN" c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    else
        cmd=("$PAR2CMD_BIN" c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    fi
    if [[ "$THREADS" != "0" ]]; then
        cmd+=("-t$THREADS")
    fi

    local start_ns end_ns status wall_seconds
    start_ns="$(date +%s%N)"
    set +e
    (
        cd "$run_dir"
        sources=(./*.bin)
        perf stat -x, -o "$perf_file" -e "$PERF_EVENTS" -- "${cmd[@]}" out.par2 "${sources[@]}" >/dev/null
    )
    status="$?"
    set -e
    end_ns="$(date +%s%N)"
    wall_seconds="$(awk -v s="$start_ns" -v e="$end_ns" 'BEGIN { printf "%.6f", (e - s) / 1000000000 }')"

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
        printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
            "$case_label" "$tool" "$run_number" "$status" \
            "$instructions" "$cycles" "$branches" "$branch_misses" \
            "$cache_references" "$cache_misses" "$task_clock" \
            "$context_switches" "$cpu_migrations" "$page_faults" "$wall_seconds" >> "$RAW_CSV"
    fi

    rm -rf "$run_dir"
    return "$status"
}

summarize_results() {
    python3 - "$RAW_CSV" "$SUMMARY_MD" "$ITERATIONS" "$WARMUP_RUNS" "$THREADS" "$REDUNDANCY" "$RECOVERY_FILES" <<'PY'
import csv
import math
import statistics
import sys
from collections import defaultdict

raw_csv, summary_md, iterations, warmups, threads, redundancy, recovery_files = sys.argv[1:]
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
    out.write("# PAR2 Create Perf Benchmark\n\n")
    out.write(f"- measured runs per tool/case: {iterations}\n")
    out.write(f"- warmup runs per tool/case: {warmups}\n")
    out.write(f"- threads: {threads}\n")
    out.write(f"- redundancy: {redundancy}%\n")
    out.write(f"- recovery files: {recovery_files}\n")
    out.write("- primary signal: Linux perf `instructions`\n\n")

    out.write("## Relative Summary\n\n")
    out.write("| case | par2rs instructions | turbo instructions | par2rs/turbo | par2rs cycles | turbo cycles | par2rs/turbo cycles |\n")
    out.write("|---|---:|---:|---:|---:|---:|---:|\n")
    for case in cases:
        p_instr = stats(number(r, "instructions") for r in groups[(case, "par2rs")])
        t_instr = stats(number(r, "instructions") for r in groups[(case, "turbo")])
        p_cycles = stats(number(r, "cycles") for r in groups[(case, "par2rs")])
        t_cycles = stats(number(r, "cycles") for r in groups[(case, "turbo")])
        instr_ratio = p_instr[0] / t_instr[0] if p_instr and t_instr and t_instr[0] else None
        cycle_ratio = p_cycles[0] / t_cycles[0] if p_cycles and t_cycles and t_cycles[0] else None
        out.write(
            f"| {case} | {fmt(p_instr[0] if p_instr else None)} | {fmt(t_instr[0] if t_instr else None)} | "
            f"{fmt(instr_ratio)} | {fmt(p_cycles[0] if p_cycles else None)} | {fmt(t_cycles[0] if t_cycles else None)} | {fmt(cycle_ratio)} |\n"
        )

    out.write("\n## Detailed Stats\n\n")
    out.write("| case | tool | metric | n | mean | stddev | cv % | min | max |\n")
    out.write("|---|---|---|---:|---:|---:|---:|---:|---:|\n")
    for case in cases:
        for tool in ("par2rs", "turbo"):
            group = groups[(case, tool)]
            failures = [r for r in group if r["status"] != "0"]
            if failures:
                out.write(f"| {case} | {tool} | failures | {len(failures)} | status != 0 |  |  |  |  |\n")
            for key, label in metrics:
                s = stats(number(r, key) for r in group)
                if s is None:
                    out.write(f"| {case} | {tool} | {label} | 0 | n/a | n/a | n/a | n/a | n/a |\n")
                    continue
                mean, stdev, cv, min_v, max_v, n = s
                out.write(f"| {case} | {tool} | {label} | {n} | {fmt(mean)} | {fmt(stdev)} | {cv:.2f} | {fmt(min_v)} | {fmt(max_v)} |\n")
PY
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

    for tool in par2rs turbo; do
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

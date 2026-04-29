#!/usr/bin/env bash
# Compare par2rs create against par2cmdline-turbo.
#
# Wall time is the pass/fail signal for the JIT-vs-turbo requirement. Linux perf
# hardware counters are captured as diagnostics to explain timing differences.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PLATFORM="$(uname -s)"
ARCH="$(uname -m)"

ITERATIONS="${ITERATIONS:-30}"
WARMUP_RUNS="${WARMUP_RUNS:-3}"
REDUNDANCY="${REDUNDANCY:-10}"
RECOVERY_FILES="${RECOVERY_FILES:-8}"
FIRST_RECOVERY_BLOCK="${FIRST_RECOVERY_BLOCK:-}"
THREADS="${THREADS:-1}"
PAR2CMD_BIN="${PAR2CMD_BIN:-par2}"
PAR2RS_BIN="${PAR2RS_BIN:-$PROJECT_ROOT/target/release/par2}"
RESULTS_ROOT="${RESULTS_ROOT:-$PROJECT_ROOT/target/perf-results/create}"
KEEP_WORK="${KEEP_WORK:-0}"
RUN_FLAMEGRAPH="${RUN_FLAMEGRAPH:-1}"
PROFILE_CASE="${PROFILE_CASE:-}"
PROFILE_TOOL="${PROFILE_TOOL:-}"
PROFILE_FREQUENCY="${PROFILE_FREQUENCY:-997}"
PERF_EVENTS="${PERF_EVENTS:-instructions,cycles,branches,branch-misses,cache-references,cache-misses,task-clock,context-switches,cpu-migrations,page-faults}"
CACHE_PROFILE="${CACHE_PROFILE:-0}"
CACHE_PROFILE_TOOL="${CACHE_PROFILE_TOOL:-}"
CACHE_PROFILE_JIT_DUMPS="${CACHE_PROFILE_JIT_DUMPS:-1}"
CACHE_LOAD_LATENCY="${CACHE_LOAD_LATENCY:-30}"
CACHE_STAT_EVENTS="${CACHE_STAT_EVENTS:-instructions,cycles,L1-dcache-loads,L1-dcache-load-misses,LLC-loads,LLC-load-misses,dTLB-loads,dTLB-load-misses,cache-references,cache-misses}"
VERIFY_OUTPUTS="${VERIFY_OUTPUTS:-1}"
VERIFY_REPAIR="${VERIFY_REPAIR:-smoke}"
INCLUDE_PSHUFB="${INCLUDE_PSHUFB:-0}"
DEFAULT_BENCHMARK_TOOLS=(
    par2rs-xor-jit
    turbo-auto
)
if [[ "$INCLUDE_PSHUFB" == "1" ]]; then
    DEFAULT_BENCHMARK_TOOLS=(par2rs-pshufb "${DEFAULT_BENCHMARK_TOOLS[@]}")
fi

# label:file_count:file_size_mib:block_size_bytes
DEFAULT_CASES="single_256m:1:256:1048576,multi_1g:64:16:1048576,single_5g:1:5120:1048576"
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
  FIRST_RECOVERY_BLOCK=N
                     pass -f N to both tools; default unset
  PAR2CMD_BIN=PATH   par2cmdline-turbo binary (default: $PAR2CMD_BIN)
  INCLUDE_PSHUFB=1    include the par2rs PSHUFB baseline (default: $INCLUDE_PSHUFB)
  WORK_ROOT=DIR      generated corpus and PAR2 work directory
                     default: result run directory/work
  CORPUS_ROOT=DIR    reusable corpus root, separate from per-run work/output dirs
                     default: under WORK_ROOT/<case>/corpus
  VERIFY_OUTPUTS=0   skip cross-tool verification after each create
  VERIFY_REPAIR=MODE run destructive cross-tool repair validation: smoke, 1, or 0
                     default: $VERIFY_REPAIR
  Required tools depend on platform/arch.
  Forced JIT runs set PAR2RS_CREATE_XOR_JIT_FALLBACK=error.
  RUN_FLAMEGRAPH=0   skip par2rs flamegraph generation
  PROFILE_CASE=LABEL profile this CASES label; default profiles the last case
  PROFILE_TOOL=TOOL   par2rs tool variant to profile (default: platform-specific)
  PROFILE_FREQUENCY=N sampling frequency for flamegraph (default: $PROFILE_FREQUENCY)
  CACHE_PROFILE=1    run Linux cache-miss attribution for the profiled case
  CACHE_PROFILE_TOOL=TOOL par2rs variant for cache profile (default: PROFILE_TOOL)
  CACHE_PROFILE_JIT_DUMPS=0 skip xor-jit byte dumps during cache profiles (default: $CACHE_PROFILE_JIT_DUMPS)
                     also disables per-overwrite coefficient perf-map labels
  CACHE_LOAD_LATENCY=N minimum load latency for precise mem-load sampling (default: $CACHE_LOAD_LATENCY)
  CACHE_STAT_EVENTS=EVENTS comma-separated cache counter list
  KEEP_WORK=1        keep generated benchmark work directory

Example:
  nix develop --command env ITERATIONS=10 THREADS=16 PROFILE_CASE=single_5g \\
    WORK_ROOT="\$HOME/uncompressed/par2rs-create-perf/manual-run" \\
    CORPUS_ROOT="\$HOME/uncompressed/par2rs-create-perf/corpus-cache" \\
    RECOVERY_FILES=8 FIRST_RECOVERY_BLOCK=1 \\
    CASES='single_256m:1:256:1048576,multi_1g:64:16:1048576,single_5g:1:5120:1048576' \\
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

tool_supported() {
    local tool="$1"

    case "$tool" in
        turbo-auto|par2rs-auto|par2rs-scalar)
            return 0
            ;;
        par2rs-pshufb|par2rs-xor-jit|par2rs-xor-jit-port)
            [[ "$ARCH" == "x86_64" ]]
            return
            ;;
        *)
            return 1
            ;;
    esac
}

BENCHMARK_TOOLS=()
for tool in "${DEFAULT_BENCHMARK_TOOLS[@]}"; do
    if tool_supported "$tool"; then
        BENCHMARK_TOOLS+=("$tool")
    fi
done

if [[ "${#BENCHMARK_TOOLS[@]}" == "1" && "${BENCHMARK_TOOLS[0]}" == "turbo-auto" ]]; then
    BENCHMARK_TOOLS=(par2rs-auto turbo-auto)
fi

if [[ -z "$PROFILE_TOOL" ]]; then
    if tool_supported "par2rs-xor-jit"; then
        PROFILE_TOOL="par2rs-xor-jit"
    else
        for tool in "${BENCHMARK_TOOLS[@]}"; do
            if [[ "$tool" == par2rs-* ]]; then
                PROFILE_TOOL="$tool"
                break
            fi
        done
    fi
fi

if ! tool_supported "$PROFILE_TOOL"; then
    echo "error: PROFILE_TOOL '$PROFILE_TOOL' is unsupported on $PLATFORM/$ARCH" >&2
    exit 1
fi

if [[ -z "$CACHE_PROFILE_TOOL" ]]; then
    CACHE_PROFILE_TOOL="$PROFILE_TOOL"
fi
if ! tool_supported "$CACHE_PROFILE_TOOL"; then
    echo "error: CACHE_PROFILE_TOOL '$CACHE_PROFILE_TOOL' is unsupported on $PLATFORM/$ARCH" >&2
    exit 1
fi

need_tool python3
need_tool dd
need_tool cmp
need_tool "$PAR2CMD_BIN"

if [[ "$PLATFORM" == "Linux" ]]; then
    need_tool perf
    if ! perf stat -x, -e instructions -- true >/dev/null 2>&1; then
        echo "error: perf cannot read the instructions counter." >&2
        echo "Check kernel.perf_event_paranoid or run in an environment with perf permissions." >&2
        exit 1
    fi
fi

mkdir -p "$RESULTS_ROOT"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
RUN_ROOT="$RESULTS_ROOT/run-$TIMESTAMP"
WORK_ROOT="${WORK_ROOT:-$RUN_ROOT/work}"
CORPUS_ROOT="${CORPUS_ROOT:-}"
RAW_CSV="$RUN_ROOT/raw.csv"
SUMMARY_MD="$RUN_ROOT/summary.md"
SMOKE_CSV="$RUN_ROOT/smoke.csv"
SMOKE_SUMMARY_MD="$RUN_ROOT/smoke-summary.md"
PRESERVE_WORK=0
mkdir -p "$RUN_ROOT" "$WORK_ROOT"

cleanup() {
    if [[ "$KEEP_WORK" != "1" && "$PRESERVE_WORK" != "1" ]]; then
        rm -rf "$WORK_ROOT"
    fi
}
trap cleanup EXIT

echo "=== par2rs create perf benchmark ==="
echo "results: $RUN_ROOT"
echo "work:    $WORK_ROOT"
if [[ -n "$CORPUS_ROOT" ]]; then
    echo "corpus:  $CORPUS_ROOT"
fi
echo "iterations: $ITERATIONS measured, $WARMUP_RUNS warmup"
echo "cases: $CASES"
echo "threads: $THREADS"
echo "recovery files: $RECOVERY_FILES"
if [[ -n "$FIRST_RECOVERY_BLOCK" ]]; then
    echo "first recovery block: $FIRST_RECOVERY_BLOCK"
fi
echo "platform: $PLATFORM/$ARCH"
echo "tools: ${BENCHMARK_TOOLS[*]}"
echo "profile tool: $PROFILE_TOOL"
if [[ "$CACHE_PROFILE" == "1" ]]; then
    echo "cache profile tool: $CACHE_PROFILE_TOOL"
fi
echo

probe_turbo_method() {
    local probe_dir="$RUN_ROOT/turbo-method-probe"
    local stdout_file="$RUN_ROOT/turbo-method-probe.stdout"
    local stderr_file="$RUN_ROOT/turbo-method-probe.stderr"
    rm -rf "$probe_dir"
    mkdir -p "$probe_dir"

    python3 - "$probe_dir/file.bin" <<'PY'
import sys

with open(sys.argv[1], "wb") as f:
    for idx in range(1024):
        f.write(bytes([(idx * 17 + 3) & 0xFF]) * 1024)
PY

    local probe_recovery_files="$RECOVERY_FILES"
    if [[ -z "$probe_recovery_files" || "$probe_recovery_files" -lt 1 ]]; then
        probe_recovery_files=1
    fi
    local -a probe_cmd=("$PAR2CMD_BIN" c -v -s1048576 -r10 "-n$probe_recovery_files")
    if [[ -n "$FIRST_RECOVERY_BLOCK" ]]; then
        probe_cmd+=("-f$FIRST_RECOVERY_BLOCK")
    fi
    if [[ "$THREADS" != "0" ]]; then
        probe_cmd+=("-t$THREADS")
    fi

    if ! (cd "$probe_dir" && "${probe_cmd[@]}" out.par2 file.bin >"$stdout_file" 2>"$stderr_file"); then
        rm -rf "$probe_dir"
        printf 'unknown'
        return 0
    fi

    local method
    method="$(awk -F': ' '/Multiply method:/ {print $2; exit}' "$stdout_file" "$stderr_file")"
    rm -rf "$probe_dir"
    printf '%s' "${method:-unknown}"
}

echo "Building par2rs release binary..."
(cd "$PROJECT_ROOT" && cargo build --release --bin par2 --quiet)

write_csv_header() {
    local csv_file="$1"
    printf 'case,tool,selected_method,run,status,validation_status,instructions,cycles,branches,branch_misses,cache_references,cache_misses,task_clock_msec,context_switches,cpu_migrations,page_faults,wall_seconds\n' > "$csv_file"
}

write_csv_header "$RAW_CSV"
write_csv_header "$SMOKE_CSV"

TURBO_OBSERVED_METHOD="$(probe_turbo_method)"
echo "turbo observed method: $TURBO_OBSERVED_METHOD"
echo

now_ns() {
    python3 - <<'PY'
import time
print(time.time_ns())
PY
}

split_cases() {
    tr ',' '\n' <<< "$CASES"
}

corpus_dir_for_case() {
    local label="$1"
    if [[ -n "$CORPUS_ROOT" ]]; then
        printf '%s/%s/corpus\n' "$CORPUS_ROOT" "$label"
    else
        printf '%s/%s/corpus\n' "$WORK_ROOT" "$label"
    fi
}

make_corpus() {
    local corpus_dir="$1"
    local file_count="$2"
    local file_size_mib="$3"

    mkdir -p "$corpus_dir"
    local i
    for i in $(seq 1 "$file_count"); do
        local file
        file="$corpus_dir/file_$(printf '%04d' "$i").bin"
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
    if [[ ! -f "$perf_file" ]]; then
        return 0
    fi
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
            par2rs-auto)
                selected_method="auto"
                env_prefix=()
                ;;
            par2rs-pshufb)
                selected_method="pshufb"
                env_prefix=(env PAR2RS_CREATE_GF16=pshufb)
                ;;
            par2rs-xor-jit|par2rs-xor-jit-port)
                selected_method="xor-jit"
                env_prefix=(env PAR2RS_CREATE_GF16=xor-jit PAR2RS_CREATE_XOR_JIT_FALLBACK=error)
                ;;
            par2rs-scalar)
                selected_method="scalar"
                env_prefix=(env PAR2RS_CREATE_GF16=scalar)
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
    if [[ -n "$FIRST_RECOVERY_BLOCK" ]]; then
        cmd+=("-f$FIRST_RECOVERY_BLOCK")
    fi
    if [[ "$THREADS" != "0" ]]; then
        cmd+=("-t$THREADS")
    fi
    local start_ns end_ns status validation_status wall_seconds
    start_ns="$(now_ns)"
    set +e
    if [[ "$PLATFORM" == "Linux" ]]; then
        (
            cd "$run_dir"
            sources=(./*.bin)
            perf stat -x, -o "$perf_file" -e "$PERF_EVENTS" -- "${env_prefix[@]}" "${cmd[@]}" out.par2 "${sources[@]}" >/dev/null
        )
    else
        (
            cd "$run_dir"
            sources=(./*.bin)
            "${env_prefix[@]}" "${cmd[@]}" out.par2 "${sources[@]}" >/dev/null
        )
    fi
    status="$?"
    set -e
    end_ns="$(now_ns)"
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

    python3 - "$raw_csv" "$summary_md" "$iterations" "$warmups" "$THREADS" "$REDUNDANCY" "$RECOVERY_FILES" "$FIRST_RECOVERY_BLOCK" "$title" "$TURBO_OBSERVED_METHOD" <<'PY'
import csv
import math
import statistics
import sys
from collections import defaultdict

raw_csv, summary_md, iterations, warmups, threads, redundancy, recovery_files, first_recovery_block, title, turbo_observed_method = sys.argv[1:]
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

def metric_mean(case, tool, key):
    group = groups.get((case, tool), [])
    s = stats(number(r, key) for r in group)
    return s[0] if s else None

def ratio(num, den):
    if num is None or den is None or den == 0:
        return None
    return num / den

cases = sorted({row["case"] for row in rows})

with open(summary_md, "w") as out:
    out.write(f"# {title}\n\n")
    out.write(f"- measured runs per tool/case: {iterations}\n")
    out.write(f"- warmup runs per tool/case: {warmups}\n")
    out.write(f"- threads: {threads}\n")
    out.write(f"- redundancy: {redundancy}%\n")
    out.write(f"- recovery files: {recovery_files}\n")
    out.write(f"- first recovery block: {first_recovery_block or 'default'}\n")
    out.write(f"- turbo observed method: {turbo_observed_method}\n")
    out.write("- validation: par2rs output verified/repaired by turbo; turbo output verified/repaired by par2rs\n")
    out.write("- primary signal: wall seconds; Linux perf counters are recorded for diagnostics when available\n\n")

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

    out.write("\n## XOR-JIT Ratios\n\n")
    out.write("| case | wall / turbo | instructions / turbo | cache misses / turbo |\n")
    out.write("|---|---:|---:|---:|\n")
    for case in cases:
        xor_wall = metric_mean(case, "par2rs-xor-jit", "wall_seconds")
        turbo_wall = metric_mean(case, "turbo-auto", "wall_seconds")
        xor_instr = metric_mean(case, "par2rs-xor-jit", "instructions")
        turbo_instr = metric_mean(case, "turbo-auto", "instructions")
        xor_cache = metric_mean(case, "par2rs-xor-jit", "cache_misses")
        turbo_cache = metric_mean(case, "turbo-auto", "cache_misses")
        out.write(
            f"| {case} | {fmt(ratio(xor_wall, turbo_wall))} | "
            f"{fmt(ratio(xor_instr, turbo_instr))} | {fmt(ratio(xor_cache, turbo_cache))} |\n"
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

enforce_pass_criteria() {
    local raw_csv="${1:-$RAW_CSV}"

    if ! tool_supported "par2rs-xor-jit"; then
        echo "Skipping XOR-JIT pass criteria on $PLATFORM/$ARCH."
        return 0
    fi

    python3 - "$raw_csv" <<'PY'
import csv
import math
import statistics
import sys
from collections import defaultdict

raw_csv = sys.argv[1]
required = ["par2rs-xor-jit", "turbo-auto"]
groups = defaultdict(list)
failures = []

with open(raw_csv, newline="") as f:
    for row in csv.DictReader(f):
        if row.get("status") != "0" or row.get("validation_status") != "0":
            failures.append(row)
        groups[(row["case"], row["tool"])].append(row)

def mean_wall(rows):
    values = []
    for row in rows:
        try:
            value = float(row.get("wall_seconds", ""))
        except ValueError:
            continue
        if math.isfinite(value):
            values.append(value)
    return statistics.fmean(values) if values else None

errors = []
if failures:
    for row in failures:
        errors.append(
            f"{row['case']}/{row['tool']} run {row['run']} status={row.get('status')} "
            f"validation={row.get('validation_status')}"
        )

cases = sorted({case for case, _tool in groups})
for case in cases:
    missing = [tool for tool in required if not groups.get((case, tool))]
    if missing:
        errors.append(f"{case}: missing required tools: {', '.join(missing)}")
        continue

    turbo = mean_wall(groups[(case, "turbo-auto")])
    if turbo is None:
        errors.append(f"{case}: missing turbo-auto wall_seconds")
        continue

    tool = "par2rs-xor-jit"
    wall = mean_wall(groups[(case, tool)])
    if wall is None:
        errors.append(f"{case}: missing {tool} wall_seconds")
    elif wall > turbo:
        errors.append(
            f"{case}: {tool} mean wall {wall:.6f}s is slower than turbo-auto {turbo:.6f}s"
        )

if errors:
    print("error: XOR-JIT performance pass criteria failed", file=sys.stderr)
    for error in errors:
        print(f"  - {error}", file=sys.stderr)
    sys.exit(1)

print("XOR-JIT performance pass criteria passed.")
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
        local corpus_dir
        corpus_dir="$(corpus_dir_for_case "$label")"
        echo "Smoke preparing '$label': ${file_count}x${file_size_mib}MiB, block size ${block_size}"
        make_corpus "$corpus_dir" "$file_count" "$file_size_mib"

        for tool in "${BENCHMARK_TOOLS[@]}"; do
            echo "Smoke measured: $label / $tool"
            run_create "$label" "$tool" "smoke" "$block_size" "$corpus_dir" 1 "$SMOKE_CSV"
        done
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

par2rs_tool_env() {
    local tool="$1"
    local -a env_args=(env)

    case "$tool" in
        par2rs-auto)
            ;;
        par2rs-pshufb)
            env_args+=(PAR2RS_CREATE_GF16=pshufb)
            ;;
        par2rs-xor-jit|par2rs-xor-jit-port)
            env_args+=(PAR2RS_CREATE_GF16=xor-jit PAR2RS_CREATE_XOR_JIT_FALLBACK=error)
            ;;
        par2rs-scalar)
            env_args+=(PAR2RS_CREATE_GF16=scalar)
            ;;
        *)
            echo "error: PROFILE_TOOL must be a par2rs variant, got: $tool" >&2
            return 1
            ;;
    esac
    env_args+=(PAR2RS_XOR_JIT_PERF_MAP=1)
    printf '%s\0' "${env_args[@]}"
}

par2rs_tool_env_args() {
    local tool="$1"
    local -a env_args=()

    case "$tool" in
        par2rs-auto)
            ;;
        par2rs-pshufb)
            env_args+=(PAR2RS_CREATE_GF16=pshufb)
            ;;
        par2rs-xor-jit|par2rs-xor-jit-port)
            env_args+=(PAR2RS_CREATE_GF16=xor-jit PAR2RS_CREATE_XOR_JIT_FALLBACK=error)
            ;;
        par2rs-scalar)
            env_args+=(PAR2RS_CREATE_GF16=scalar)
            ;;
        *)
            echo "error: PROFILE_TOOL must be a par2rs variant, got: $tool" >&2
            return 1
            ;;
    esac
    env_args+=(PAR2RS_XOR_JIT_PERF_MAP=1)
    if [[ "${#env_args[@]}" -gt 0 ]]; then
        printf '%s\0' "${env_args[@]}"
    fi
}

generate_flamegraph() {
    local case_spec="$1"
    IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
    local case_dir="$WORK_ROOT/$label"
    local corpus_dir
    corpus_dir="$(corpus_dir_for_case "$label")"
    local flamegraph_file="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL-flamegraph.svg"
    local output_file="$case_dir/flamegraph-out.par2"

    if [[ "$RUN_FLAMEGRAPH" != "1" ]]; then
        return 0
    fi

    rm -f "$case_dir"/flamegraph-out*.par2
    local -a args=(c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    if [[ -n "$FIRST_RECOVERY_BLOCK" ]]; then
        args+=("-f$FIRST_RECOVERY_BLOCK")
    fi
    if [[ "$THREADS" != "0" ]]; then
        args+=("-t$THREADS")
    fi
    args+=("$output_file")
    local file
    for file in "$corpus_dir"/*.bin; do
        args+=("$file")
    done

    case "$PLATFORM" in
        Linux)
            if ! command -v inferno-collapse-perf >/dev/null 2>&1 || ! command -v inferno-flamegraph >/dev/null 2>&1; then
                if command -v cargo >/dev/null 2>&1; then
                    echo "inferno not found; installing with cargo install inferno..."
                    cargo install inferno
                else
                    echo "warning: inferno is unavailable; skipping flamegraph" >&2
                    return 0
                fi
            fi

            echo
            echo "Building par2rs profiling binary with frame pointers..."
            (
                cd "$PROJECT_ROOT"
                RUSTFLAGS="-C target-cpu=native -C force-frame-pointers=yes" \
                    cargo build --profile profiling --bin par2 --quiet
            )

            local profiling_bin="$PROJECT_ROOT/target/profiling/par2"
            local perf_data="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.perf.data"
            local perf_summary="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.perf-report.txt"
            local hotspots="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.hotspots.txt"
            local -a env_prefix
            mapfile -d '' -t env_prefix < <(par2rs_tool_env "$PROFILE_TOOL")

            echo "Recording par2rs flamegraph for case '$label' / $PROFILE_TOOL..."
            if ! perf record -g --call-graph fp -F "$PROFILE_FREQUENCY" -o "$perf_data" -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
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
            ;;
        Darwin)
            if ! command -v xctrace >/dev/null 2>&1; then
                echo "warning: xctrace is unavailable; skipping flamegraph" >&2
                return 0
            fi
            if ! command -v inferno-collapse-xctrace >/dev/null 2>&1; then
                echo "warning: inferno-collapse-xctrace is unavailable; skipping flamegraph" >&2
                return 0
            fi
            if ! command -v inferno-flamegraph >/dev/null 2>&1; then
                echo "warning: inferno-flamegraph is unavailable; skipping flamegraph" >&2
                return 0
            fi

            local -a env_vars
            local xctrace_bin xctrace_dir developer_dir
            local trace_file xml_file folded_file
            mapfile -d '' -t env_vars < <(par2rs_tool_env_args "$PROFILE_TOOL")
            xctrace_bin="$(command -v xctrace || true)"
            if [[ -z "$xctrace_bin" ]]; then
                xctrace_bin="$(xcrun --find xctrace 2>/dev/null || true)"
            fi
            if [[ -z "$xctrace_bin" ]]; then
                echo "warning: xctrace is unavailable; skipping flamegraph" >&2
                return 0
            fi
            xctrace_dir="$(dirname "$xctrace_bin")"
            developer_dir="/Applications/Xcode.app/Contents/Developer"
            if [[ ! -d "$developer_dir" ]]; then
                developer_dir="$(dirname "$(dirname "$xctrace_dir")")"
            fi
            trace_file="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.trace"
            xml_file="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.xml"
            folded_file="$RUN_ROOT/par2rs-create-$label-$PROFILE_TOOL.folded"

            echo
            echo "Building par2rs profiling binary with frame pointers..."
            (
                cd "$PROJECT_ROOT"
                RUSTFLAGS="-C target-cpu=native -C force-frame-pointers=yes" \
                    cargo build --profile profiling --bin par2 --quiet
            ) || {
                echo "warning: flamegraph generation failed; benchmark results are still available" >&2
                return 0
            }

            local profiling_bin="$PROJECT_ROOT/target/profiling/par2"
            local -a xctrace_cmd=(
                "$xctrace_bin" record
                --template "Time Profiler"
                --output "$trace_file"
                --target-stdout /dev/null
                --launch --
                "$profiling_bin"
            )
            local env_var
            for env_var in "${env_vars[@]}"; do
                xctrace_cmd+=(--env "$env_var")
            done
            xctrace_cmd+=("${args[@]}")

            echo "Recording par2rs flamegraph for case '$label' / $PROFILE_TOOL with xctrace..."
            env DEVELOPER_DIR="$developer_dir" PATH="$xctrace_dir:$PATH" \
                "${xctrace_cmd[@]}" >/dev/null || {
                echo "warning: xctrace recording failed; benchmark results are still available" >&2
                return 0
            }

            echo "Exporting xctrace profile..."
            env DEVELOPER_DIR="$developer_dir" PATH="$xctrace_dir:$PATH" \
                xctrace export --input "$trace_file" \
                --xpath '/trace-toc/*/data/table[@schema="time-profile"]' > "$xml_file" || {
                echo "warning: xctrace export failed; benchmark results are still available" >&2
                return 0
            }

            echo "Collapsing xctrace stacks..."
            if ! inferno-collapse-xctrace "$xml_file" > "$folded_file"; then
                echo "warning: xctrace stack collapse failed; benchmark results are still available" >&2
                return 0
            fi

            echo "Rendering flamegraph SVG..."
            if ! inferno-flamegraph "$folded_file" > "$flamegraph_file"; then
                echo "warning: inferno flamegraph rendering failed; benchmark results are still available" >&2
                return 0
            fi

            echo "flamegraph: $flamegraph_file"
            echo "trace:      $trace_file"
            echo "xml:        $xml_file"
            echo "folded:     $folded_file"
            ;;
        *)
            echo "warning: flamegraph generation is unsupported on $PLATFORM; skipping flamegraph" >&2
            ;;
    esac
}

generate_cache_profile() {
    local case_spec="$1"
    IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
    local case_dir="$WORK_ROOT/$label"
    local corpus_dir
    corpus_dir="$(corpus_dir_for_case "$label")"

    if [[ "$CACHE_PROFILE" != "1" ]]; then
        return 0
    fi
    if [[ "$PLATFORM" != "Linux" ]]; then
        echo "warning: cache profiling is currently Linux-only; skipping cache profile" >&2
        return 0
    fi

    echo
    echo "Preparing cache-miss attribution for case '$label' / $CACHE_PROFILE_TOOL..."
    (
        cd "$PROJECT_ROOT"
        RUSTFLAGS="-C target-cpu=native -C force-frame-pointers=yes" \
            cargo build --profile profiling --bin par2 --quiet
    )

    local profiling_bin="$PROJECT_ROOT/target/profiling/par2"
    local output_file="$case_dir/cache-profile-out.par2"
    local cache_stat="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.cache-stat.txt"
    local cache_miss_data="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.cache-misses.data"
    local cache_miss_report="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.cache-misses-report.txt"
    local cache_miss_script="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.cache-misses-script.txt"
    local perf_mem_data="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.perf-mem.data"
    local perf_mem_report="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.perf-mem-report.txt"
    local precise_data="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.mem-loads.data"
    local precise_report="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.mem-loads-report.txt"
    local ibs_data="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.ibs-op.data"
    local ibs_report="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.ibs-op-report.txt"
    local ibs_script="$RUN_ROOT/par2rs-create-$label-$CACHE_PROFILE_TOOL.ibs-op-script.txt"
    local -a env_prefix
    local -a args=(c -q -q "-s$block_size" "-r$REDUNDANCY" "-n$RECOVERY_FILES")
    if [[ -n "$FIRST_RECOVERY_BLOCK" ]]; then
        args+=("-f$FIRST_RECOVERY_BLOCK")
    fi

    mapfile -d '' -t env_prefix < <(par2rs_tool_env "$CACHE_PROFILE_TOOL")
    if [[ "$CACHE_PROFILE_JIT_DUMPS" == "1" ]]; then
        env_prefix+=(PAR2RS_XOR_JIT_DUMP_DIR="$RUN_ROOT/xor-jit-dumps")
    else
        env_prefix+=(PAR2RS_XOR_JIT_PERF_COEFF_LABELS=0)
    fi
    if [[ "$THREADS" != "0" ]]; then
        args+=("-t$THREADS")
    fi
    args+=("$output_file")
    local file
    for file in "$corpus_dir"/*.bin; do
        args+=("$file")
    done

    echo "Collecting targeted cache counters..."
    rm -f "$case_dir"/cache-profile-out*.par2
    if ! perf stat -o "$cache_stat" -d -d -d -e "$CACHE_STAT_EVENTS" -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
        echo "warning: cache perf stat failed; see $cache_stat" >&2
    fi

    echo "Recording cache-miss callgraph samples..."
    rm -f "$case_dir"/cache-profile-out*.par2
    if perf record -g --call-graph fp -e cache-misses:u -o "$cache_miss_data" -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
        perf report -i "$cache_miss_data" --stdio --sort comm,dso,symbol > "$cache_miss_report" 2>/dev/null || true
        perf script -i "$cache_miss_data" > "$cache_miss_script" 2>/dev/null || true
        echo "cache-misses data:  $cache_miss_data"
        echo "cache-misses text:  $cache_miss_report"
        echo "cache-misses script:$cache_miss_script"
    else
        echo "warning: cache-miss callgraph sampling failed; see perf permissions/counter availability" >&2
        rm -f "$cache_miss_data"
    fi

    echo "Recording data-source samples with perf mem..."
    rm -f "$case_dir"/cache-profile-out*.par2
    if perf mem record -o "$perf_mem_data" --call-graph fp -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
        echo "Generating perf mem report..."
        perf mem report -i "$perf_mem_data" --stdio --sort symbol,dso,mem,local_weight > "$perf_mem_report" 2>/dev/null || true
        echo "perf mem data:   $perf_mem_data"
        echo "perf mem report: $perf_mem_report"
    else
        echo "warning: perf mem record failed; falling back to precise mem-load sampling" >&2
        rm -f "$perf_mem_data"
    fi

    echo "Recording high-latency load samples..."
    rm -f "$case_dir"/cache-profile-out*.par2
    if perf record -g --call-graph fp -e "cpu/mem-loads,ldlat=${CACHE_LOAD_LATENCY}/P" -o "$precise_data" -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
        echo "Generating high-latency load report..."
        perf report -i "$precise_data" --stdio --sort comm,dso,symbol > "$precise_report" 2>/dev/null || true
        echo "mem-loads data:  $precise_data"
        echo "mem-loads text:  $precise_report"
    else
        echo "warning: precise mem-load sampling failed; trying AMD IBS op sampling" >&2
        rm -f "$precise_data"
        if [[ "$(perf list 2>/dev/null)" == *ibs_op* ]]; then
            local old_perf_paranoid=""
            if [[ -r /proc/sys/kernel/perf_event_paranoid ]]; then
                old_perf_paranoid="$(cat /proc/sys/kernel/perf_event_paranoid)"
            fi
            if [[ -n "$old_perf_paranoid" && "$old_perf_paranoid" != "-1" ]]; then
                if sudo -n true >/dev/null 2>&1; then
                    sudo -n sysctl -w kernel.perf_event_paranoid=-1 >/dev/null || true
                else
                    echo "warning: AMD IBS needs system-wide perf access; sudo -n is unavailable" >&2
                fi
            fi

            rm -f "$case_dir"/cache-profile-out*.par2
            if perf record -g --call-graph fp -e ibs_op// -o "$ibs_data" -- "${env_prefix[@]}" "$profiling_bin" "${args[@]}" >/dev/null; then
                perf report -i "$ibs_data" --stdio --sort comm,dso,symbol > "$ibs_report" 2>/dev/null || true
                perf script -i "$ibs_data" > "$ibs_script" 2>/dev/null || true
                echo "ibs-op data:     $ibs_data"
                echo "ibs-op text:     $ibs_report"
                echo "ibs-op script:   $ibs_script"
            else
                echo "warning: AMD IBS op sampling failed; see perf permissions and IBS PMU support" >&2
                rm -f "$ibs_data"
            fi

            if [[ -n "$old_perf_paranoid" && "$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || true)" != "$old_perf_paranoid" ]]; then
                sudo -n sysctl -w kernel.perf_event_paranoid="$old_perf_paranoid" >/dev/null || true
            fi
        else
            echo "warning: precise mem-load sampling failed; event may be unsupported on this CPU/kernel" >&2
        fi
    fi

    echo "cache stat:      $cache_stat"
}

last_case=""
profile_case_spec=""
run_smoke_benchmarks

while IFS= read -r case_spec; do
    [[ -z "$case_spec" ]] && continue
    IFS=: read -r label file_count file_size_mib block_size <<< "$case_spec"
    if [[ -z "${label:-}" || -z "${file_count:-}" || -z "${file_size_mib:-}" || -z "${block_size:-}" ]]; then
        echo "error: invalid CASES entry: $case_spec" >&2
        exit 1
    fi

    last_case="$case_spec"
    if [[ -n "$PROFILE_CASE" && ( "$PROFILE_CASE" == "$label" || "$PROFILE_CASE" == "$case_spec" ) ]]; then
        profile_case_spec="$case_spec"
    fi
    case_dir="$WORK_ROOT/$label"
    corpus_dir="$(corpus_dir_for_case "$label")"
    echo "Preparing case '$label': ${file_count}x${file_size_mib}MiB, block size ${block_size}"
    make_corpus "$corpus_dir" "$file_count" "$file_size_mib"

    for tool in "${BENCHMARK_TOOLS[@]}"; do
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
if [[ -n "$PROFILE_CASE" && -z "$profile_case_spec" ]]; then
    echo "error: PROFILE_CASE '$PROFILE_CASE' did not match any CASES label or spec" >&2
    exit 1
fi
if [[ -z "$profile_case_spec" ]]; then
    profile_case_spec="$last_case"
fi
if [[ -n "$profile_case_spec" ]]; then
    generate_flamegraph "$profile_case_spec"
    generate_cache_profile "$profile_case_spec"
fi
enforce_pass_criteria "$RAW_CSV"

echo
echo "raw data: $RAW_CSV"
echo "summary:  $SUMMARY_MD"
if [[ "$KEEP_WORK" == "1" ]]; then
    echo "work dir:  $WORK_ROOT"
fi

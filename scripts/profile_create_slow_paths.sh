#!/usr/bin/env bash
# Profile representative par2rs create workloads against ParPar-backed comparison paths.
#
# The default comparison target is par2cmdline-turbo, which embeds ParPar's
# create backend. Set PARPAR_BIN to also run a standalone ParPar-compatible CLI
# if one is available in your environment.

set -euo pipefail

SCRIPT_DIR="$(dirname "$(realpath "${BASH_SOURCE[0]}")")"
PROJECT_ROOT="${PROJECT_ROOT:-$(git -C "$SCRIPT_DIR/.." rev-parse --show-toplevel)}"
PAR2RS_BIN="${PAR2RS_BIN:-$PROJECT_ROOT/target/release/par2}"
PAR2CMD_BIN="${PAR2CMD_BIN:-par2}"
PARPAR_BIN="${PARPAR_BIN:-}"
RESULTS_ROOT="${RESULTS_ROOT:-$PROJECT_ROOT/target/perf-results/create-slow-paths}"
WORK_ROOT="${WORK_ROOT:-}"
ITERATIONS="${ITERATIONS:-3}"
WARMUP_RUNS="${WARMUP_RUNS:-1}"
THREADS="${THREADS:-1}"
RECOVERY_FILES="${RECOVERY_FILES:-4}"
KEEP_WORK="${KEEP_WORK:-0}"
RUN_KERNEL_BENCH="${RUN_KERNEL_BENCH:-0}"
TRACE_IO="${TRACE_IO:-0}"
PERF_EVENTS="${PERF_EVENTS:-instructions,cache-misses,branches,branch-misses,task-clock,context-switches}"
DATE_BIN="${DATE_BIN:-date}"

# label:file_count:file_size_bytes:block_size:recovery_percent:memory_mb
DEFAULT_WORKLOADS="small_full_block:1:8388608:1048576:10:0,large_capped:1:100663296:67108864:10:0,many_small_files:128:65536:65536:10:0,one_large_file:1:268435456:1048576:10:0,high_recovery_count:1:33554432:1048576:50:0,forced_slim_low_memory:1:33554432:8388608:10:16"
WORKLOADS="${WORKLOADS:-$DEFAULT_WORKLOADS}"

usage() {
    cat <<EOF
Usage: $(basename "$0")

Builds par2rs, creates deterministic corpora, and profiles representative
create workloads. Results are written under RESULTS_ROOT.

Environment:
  ITERATIONS=N       measured runs per workload/tool (default: $ITERATIONS)
  WARMUP_RUNS=N      warmups per workload/tool (default: $WARMUP_RUNS)
  THREADS=N          -t value for create commands; 0 omits -t (default: $THREADS)
  RECOVERY_FILES=N   -n value for create commands (default: $RECOVERY_FILES)
  WORKLOADS=SPEC     comma-separated label:file_count:file_size:block_size:redundancy:memory_mb
  PAR2CMD_BIN=PATH   par2cmdline-turbo binary (default: $PAR2CMD_BIN)
  PARPAR_BIN=PATH    optional standalone ParPar-compatible CLI
  PERF_EVENTS=LIST   perf stat event list (default: $PERF_EVENTS)
  TRACE_IO=1         also run strace syscall-count samples when strace exists
  RUN_KERNEL_BENCH=1 run embedded ParPar Criterion comparison after full-run profiling
  KEEP_WORK=1        retain generated corpus/output directories

Output:
  raw.csv            wall time and paths to perf/profile logs
  counters.csv       parsed wall, instruction, cache, branch, and context-switch counters
  phase.csv          parsed PAR2RS_CREATE_PROFILE phase/counter output
  summary.md         per-workload median wall-time ranking
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

need_tool perl
need_tool cargo
need_tool perf
need_tool "$PAR2CMD_BIN"

if ! perf stat -x, -e instructions -- true >/dev/null 2>&1; then
    echo "error: perf cannot read hardware counters in this environment" >&2
    exit 1
fi

mkdir -p "$RESULTS_ROOT"
RUN_ID="$("$DATE_BIN" +%Y%m%d_%H%M%S)"
RUN_ROOT="$RESULTS_ROOT/run-$RUN_ID"
WORK_ROOT="${WORK_ROOT:-$RUN_ROOT/work}"
RAW_CSV="$RUN_ROOT/raw.csv"
COUNTERS_CSV="$RUN_ROOT/counters.csv"
PHASE_CSV="$RUN_ROOT/phase.csv"
SUMMARY_MD="$RUN_ROOT/summary.md"
mkdir -p "$RUN_ROOT" "$WORK_ROOT"

cleanup() {
    if [[ "$KEEP_WORK" != "1" ]]; then
        rm -rf "$WORK_ROOT"
    fi
}
trap cleanup EXIT

echo "Building par2rs release binary..."
cargo build --manifest-path "$PROJECT_ROOT/Cargo.toml" --release --bin par2 --quiet

printf 'case,tool,iteration,kind,wall_seconds,exit_status,perf_log,profile_log,io_log,work_dir\n' >"$RAW_CSV"
printf 'case,tool,iteration,wall_seconds,instructions,cache_misses,branches,branch_misses,task_clock_ms,context_switches\n' >"$COUNTERS_CSV"
printf 'case,tool,iteration,section,name,value\n' >"$PHASE_CSV"

make_corpus() {
    local corpus_dir="$1"
    local file_count="$2"
    local file_size="$3"
    rm -rf "$corpus_dir"
    mkdir -p "$corpus_dir"
    perl - "$corpus_dir" "$file_count" "$file_size" <<'PL'
use strict;
use warnings;
use bytes;

my ($root, $file_count, $file_size) = @ARGV;
my $chunk_size = 1024 * 1024;

for my $file_idx (0 .. $file_count - 1) {
    my $path = sprintf "%s/file_%05d.bin", $root, $file_idx;
    open my $fh, ">:raw", $path or die "open $path: $!";
    my $remaining = $file_size;
    my $offset = 0;
    while ($remaining > 0) {
        my $n = $remaining < $chunk_size ? $remaining : $chunk_size;
        my $data = pack "C*", map { ($file_idx * 131 + $offset + $_) & 0xFF } 0 .. $n - 1;
        print {$fh} $data or die "write $path: $!";
        $remaining -= $n;
        $offset += $n;
    }
    close $fh or die "close $path: $!";
}
PL
}

source_files_for() {
    local corpus_dir="$1"
    find "$corpus_dir" -type f -name '*.bin' -print | sort
}

run_create() {
    local case_label="$1"
    local tool="$2"
    local iteration="$3"
    local kind="$4"
    local output_dir="$5"
    local corpus_dir="$6"
    local block_size="$7"
    local redundancy="$8"
    local memory_mb="$9"

    rm -rf "$output_dir"
    mkdir -p "$output_dir"
    local output_base="$output_dir/out.par2"
    local perf_log="$output_dir/perf.log"
    local profile_log="$output_dir/profile.log"
    local io_log="$output_dir/io.log"
    local -a sources
    mapfile -t sources < <(source_files_for "$corpus_dir")

    local -a cmd env_prefix
    env_prefix=()
    case "$tool" in
        par2rs)
            env_prefix=(env PAR2RS_CREATE_PROFILE=1)
            cmd=("$PAR2RS_BIN" c -q "-s$block_size" "-r$redundancy" "-n$RECOVERY_FILES")
            ;;
        turbo-parpar)
            cmd=("$PAR2CMD_BIN" c -q "-s$block_size" "-r$redundancy" "-n$RECOVERY_FILES")
            ;;
        parpar)
            if [[ -z "$PARPAR_BIN" ]]; then
                return 0
            fi
            cmd=("$PARPAR_BIN" c -q "-s$block_size" "-r$redundancy" "-n$RECOVERY_FILES")
            ;;
        *)
            echo "error: unknown tool '$tool'" >&2
            exit 1
            ;;
    esac
    if [[ "$THREADS" != "0" ]]; then
        cmd+=("-t$THREADS")
    fi
    if [[ "$memory_mb" != "0" ]]; then
        cmd+=("-m$memory_mb")
    fi
    cmd+=("$output_base")
    cmd+=("${sources[@]}")

    local start_ns end_ns status wall
    start_ns="$("$DATE_BIN" +%s%N)"
    set +e
    "${env_prefix[@]}" perf stat -x, -e "$PERF_EVENTS" -o "$perf_log" -- "${cmd[@]}" >"$output_dir/stdout.log" 2>"$profile_log"
    status=$?
    set -e
    end_ns="$("$DATE_BIN" +%s%N)"
    wall="$(perl -e 'printf "%.9f\n", ($ARGV[1] - $ARGV[0]) / 1_000_000_000' "$start_ns" "$end_ns")"

    if [[ "$TRACE_IO" == "1" && "$kind" == "measured" ]] && command -v strace >/dev/null 2>&1; then
        rm -rf "$output_dir/io-sample"
        mkdir -p "$output_dir/io-sample"
        local io_output="$output_dir/io-sample/out.par2"
        local -a io_cmd=("${cmd[@]}")
        local output_idx=$(( ${#io_cmd[@]} - ${#sources[@]} - 1 ))
        io_cmd[$output_idx]="$io_output"
        strace -qq -c -e trace=read,pread64,lseek,write,pwrite64 -o "$io_log" "${env_prefix[@]}" "${io_cmd[@]}" >/dev/null 2>&1 || true
    fi

    printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
        "$case_label" "$tool" "$iteration" "$kind" "$wall" "$status" \
        "$perf_log" "$profile_log" "$io_log" "$output_dir" >>"$RAW_CSV"

    if [[ "$kind" == "measured" ]]; then
        perl - "$case_label" "$tool" "$iteration" "$wall" "$perf_log" "$profile_log" "$COUNTERS_CSV" "$PHASE_CSV" <<'PL'
use strict;
use warnings;

my ($case_label, $tool, $iteration, $wall, $perf_log, $profile_log, $counters_csv, $phase_csv) = @ARGV;
my %events = map { $_ => "" } qw(instructions cache-misses branches branch-misses task-clock context-switches);

if (open my $perf, "<", $perf_log) {
    while (my $line = <$perf>) {
        chomp $line;
        my @row = split /,/, $line;
        next unless @row >= 3;
        if (exists $events{$row[2]}) {
            $row[0] =~ s/,//g;
            $events{$row[2]} = $row[0];
        }
    }
    close $perf;
}

open my $counters, ">>", $counters_csv or die "open $counters_csv: $!";
print {$counters} join(",", $case_label, $tool, $iteration, $wall,
    @events{qw(instructions cache-misses branches branch-misses task-clock context-switches)}
) . "\n";
close $counters;

if (open my $profile, "<", $profile_log) {
    open my $phase, ">>", $phase_csv or die "open $phase_csv: $!";
    my $in_profile = 0;
    my $section = "";
    while (my $line = <$profile>) {
        chomp $line;
        if ($line eq "PAR2RS_CREATE_PROFILE_BEGIN") {
            $in_profile = 1;
            next;
        }
        last if $line eq "PAR2RS_CREATE_PROFILE_END";
        next unless $in_profile;
        if ($line eq "phase,seconds") {
            $section = "phase";
            next;
        }
        if ($line eq "counter,value") {
            $section = "counter";
            next;
        }
        next unless index($line, ",") >= 0;
        my ($name, $value) = split /,/, $line, 2;
        print {$phase} join(",", $case_label, $tool, $iteration, $section, $name, $value) . "\n";
    }
    close $phase;
    close $profile;
}
PL
    fi
}

IFS=',' read -r -a workload_specs <<<"$WORKLOADS"
tools=(par2rs turbo-parpar)
if [[ -n "$PARPAR_BIN" ]]; then
    tools+=(parpar)
fi

for spec in "${workload_specs[@]}"; do
    IFS=':' read -r label file_count file_size block_size redundancy memory_mb <<<"$spec"
    corpus_dir="$WORK_ROOT/$label/corpus"
    echo "Preparing workload $label..."
    make_corpus "$corpus_dir" "$file_count" "$file_size"

    for tool in "${tools[@]}"; do
        for ((i = 1; i <= WARMUP_RUNS; i++)); do
            run_create "$label" "$tool" "$i" "warmup" "$WORK_ROOT/$label/$tool/warmup-$i" "$corpus_dir" "$block_size" "$redundancy" "$memory_mb"
        done
        for ((i = 1; i <= ITERATIONS; i++)); do
            echo "Running $label / $tool / iteration $i..."
            run_create "$label" "$tool" "$i" "measured" "$WORK_ROOT/$label/$tool/run-$i" "$corpus_dir" "$block_size" "$redundancy" "$memory_mb"
        done
    done
done

perl - "$COUNTERS_CSV" "$PHASE_CSV" "$SUMMARY_MD" <<'PL'
use strict;
use warnings;

my ($counters_csv, $phase_csv, $summary_md) = @ARGV;
my %wall;
my %phase;

sub median {
    my @values = sort { $a <=> $b } @_;
    return 0 unless @values;
    my $mid = int(@values / 2);
    return @values % 2 ? $values[$mid] : ($values[$mid - 1] + $values[$mid]) / 2;
}

if (open my $counters, "<", $counters_csv) {
    my $header = <$counters>;
    while (my $line = <$counters>) {
        chomp $line;
        my @row = split /,/, $line;
        next unless @row >= 4 && $row[3] ne "";
        push @{ $wall{"$row[0]\0$row[1]"} }, $row[3] + 0;
    }
    close $counters;
}

if (open my $ph, "<", $phase_csv) {
    my $header = <$ph>;
    while (my $line = <$ph>) {
        chomp $line;
        my @row = split /,/, $line;
        next unless @row >= 6;
        next unless $row[3] eq "phase" && $row[1] eq "par2rs";
        push @{ $phase{"$row[0]\0$row[4]"} }, $row[5] + 0;
    }
    close $ph;
}

open my $out, ">", $summary_md or die "open $summary_md: $!";
print {$out} "# par2rs create slow-path profile\n\n";
print {$out} "## Median wall time\n\n";
print {$out} "| workload | tool | median seconds |\n";
print {$out} "| --- | ---: | ---: |\n";
for my $key (sort keys %wall) {
    my ($case, $tool) = split /\0/, $key;
    printf {$out} "| %s | %s | %.6f |\n", $case, $tool, median(@{ $wall{$key} });
}
print {$out} "\n## par2rs median phase time\n\n";
print {$out} "| workload | phase | median seconds |\n";
print {$out} "| --- | ---: | ---: |\n";
for my $key (sort keys %phase) {
    my ($case, $name) = split /\0/, $key;
    printf {$out} "| %s | %s | %.6f |\n", $case, $name, median(@{ $phase{$key} });
}
close $out;
PL

if [[ "$RUN_KERNEL_BENCH" == "1" ]]; then
    cargo bench --manifest-path "$PROJECT_ROOT/Cargo.toml" --features parpar-compare --bench compare_with_parpar -- --save-baseline parpar-create-slow-paths
fi

echo "Results written to $RUN_ROOT"
echo "Summary: $SUMMARY_MD"

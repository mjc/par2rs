#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${PARITY_COMPARE_USE_NIX:-1}" != "0" && "${PARITY_COMPARE_IN_NIX:-0}" != "1" ]]; then
  exec nix develop "$ROOT" -c env PARITY_COMPARE_IN_NIX=1 bash "$0" "$@"
fi

TURBO_ROOT="${TURBO_ROOT:-/home/mjc/projects/par2cmdline-turbo}"
PAR2RS_BIN_DIR="$ROOT/target/release"
WORK_DIR="${WORK_DIR:-$(mktemp -d)}"

if [[ -x "${TURBO_PAR2:-}" ]]; then
  TURBO_PAR2_CMD="$TURBO_PAR2"
elif [[ -x "$TURBO_ROOT/par2" ]]; then
  TURBO_PAR2_CMD="$TURBO_ROOT/par2"
else
  TURBO_PAR2_CMD="${TURBO_PAR2:-par2}"
fi

resolve_turbo_wrapper() {
  local explicit="$1"
  local binary="$2"
  local par2_path="$3"
  if [[ -x "$explicit" ]]; then
    printf '%s' "$explicit"
    return
  fi
  if [[ -n "$par2_path" ]]; then
    local par2_dir
    par2_dir="$(dirname "$par2_path")"
    if [[ -x "$par2_dir/$binary" ]]; then
      printf '%s' "$par2_dir/$binary"
      return
    fi
  fi
  printf '%s' "$binary"
}

TURBO_PAR2_PATH=""
if command -v "$TURBO_PAR2_CMD" >/dev/null 2>&1; then
  HAS_TURBO=1
  TURBO_PAR2_PATH="$(command -v "$TURBO_PAR2_CMD")"
else
  HAS_TURBO=0
fi
TURBO_PAR2VERIFY_CMD="$(resolve_turbo_wrapper "${TURBO_PAR2VERIFY:-}" par2verify "$TURBO_PAR2_PATH")"
TURBO_PAR2REPAIR_CMD="$(resolve_turbo_wrapper "${TURBO_PAR2REPAIR:-}" par2repair "$TURBO_PAR2_PATH")"
TURBO_PAR2CREATE_CMD="$(resolve_turbo_wrapper "${TURBO_PAR2CREATE:-}" par2create "$TURBO_PAR2_PATH")"

cleanup() {
  if [[ -z "${KEEP_WORK_DIR:-}" ]]; then
    rm -rf "$WORK_DIR"
  else
    printf 'kept work dir: %s\n' "$WORK_DIR"
  fi
}
trap cleanup EXIT

run_case() {
  local name="$1"
  shift
  printf 'case: %s\n' "$name"
  "$@"
  printf '  ok\n'
}

run_capture() {
  local dir="$1"
  local out="$2"
  shift 2
  set +e
  (cd "$dir" && "$@") >"$out.stdout" 2>"$out.stderr"
  local status=$?
  set -e
  printf '%s' "$status" >"$out.status"
}

assert_same_status() {
  local left="$1"
  local right="$2"
  local left_status right_status
  left_status="$(cat "$left.status")"
  right_status="$(cat "$right.status")"
  if [[ "$left_status" != "$right_status" ]]; then
    printf 'status mismatch: turbo=%s par2rs=%s\n' "$left_status" "$right_status" >&2
    printf 'turbo stderr:\n' >&2
    sed -n '1,80p' "$left.stderr" >&2
    printf 'par2rs stderr:\n' >&2
    sed -n '1,80p' "$right.stderr" >&2
    return 1
  fi
}

assert_zero_status() {
  local result="$1"
  if [[ "$(cat "$result.status")" != "0" ]]; then
    printf 'expected zero status for %s\n' "$result" >&2
    sed -n '1,80p' "$result.stderr" >&2
    return 1
  fi
}

assert_nonzero_status() {
  local result="$1"
  if [[ "$(cat "$result.status")" = "0" ]]; then
    printf 'expected nonzero status for %s\n' "$result" >&2
    sed -n '1,80p' "$result.stderr" >&2
    return 1
  fi
}

assert_pair_same_status() {
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_same_status "$TURBO_RESULT" "$PAR2RS_RESULT"
  fi
}

assert_pair_nonzero_status() {
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_nonzero_status "$TURBO_RESULT"
  fi
  assert_nonzero_status "$PAR2RS_RESULT"
}

assert_pair_zero_status() {
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_zero_status "$TURBO_RESULT"
  fi
  assert_zero_status "$PAR2RS_RESULT"
}

assert_hash_equal() {
  local left="$1"
  local right="$2"
  cmp -s "$left" "$right"
}

assert_same_file_presence() {
  local left="$1"
  local right="$2"
  if [[ -e "$left" && ! -e "$right" ]] || [[ ! -e "$left" && -e "$right" ]]; then
    printf 'file presence mismatch: %s vs %s\n' "$left" "$right" >&2
    return 1
  fi
}

assert_absent() {
  local path="$1"
  if [[ -e "$path" ]]; then
    printf 'expected absent: %s\n' "$path" >&2
    return 1
  fi
}

assert_file_exists() {
  local path="$1"
  if [[ ! -e "$path" ]]; then
    printf 'expected file to exist: %s\n' "$path" >&2
    return 1
  fi
}

assert_glob_absent() {
  local pattern="$1"
  local dir base
  dir="$(dirname "$pattern")"
  base="$(basename "$pattern")"
  if [[ -d "$dir" && -n "$(find "$dir" -maxdepth 1 -name "$base" -print -quit)" ]]; then
    printf 'expected no files matching: %s\n' "$pattern" >&2
    find "$dir" -maxdepth 1 -name "$base" -print >&2
    return 1
  fi
}

assert_glob_present() {
  local pattern="$1"
  local dir base
  dir="$(dirname "$pattern")"
  base="$(basename "$pattern")"
  if [[ ! -d "$dir" || -z "$(find "$dir" -maxdepth 1 -name "$base" -print -quit)" ]]; then
    printf 'expected file matching: %s\n' "$pattern" >&2
    return 1
  fi
}

assert_no_par1_recovery_files() {
  local dir="$1"
  assert_absent "$dir/testdata.par"
  assert_absent "$dir/testdata.p01"
  assert_absent "$dir/testdata.p02"
}

assert_no_par2_recovery_files() {
  local dir="$1"
  local base_stem="$2"
  assert_absent "$dir/$base_stem.par2"
  assert_glob_absent "$dir/$base_stem.vol*.par2"
}

assert_par2_set_created() {
  local dir="$1"
  local par2_file="$2"
  local stem="${par2_file%.par2}"
  assert_file_exists "$dir/$par2_file"
  assert_glob_present "$dir/$stem.vol*.par2"
}

assert_verify_success_for_created_set() {
  local tool="$1"
  local dir="$2"
  local par2_file="$3"
  shift 3
  local result="$WORK_DIR/verify-$(basename "$dir")-${par2_file%.par2}-$RANDOM"
  run_capture "$dir" "$result" "$tool" verify "$@" "$par2_file"
  assert_zero_status "$result"
}

copy_fixture_set() {
  local dir="$1"
  mkdir -p "$dir"
  cp "$ROOT/tests/fixtures/testfile" "$dir/"
  cp "$ROOT/tests/fixtures/testfile"*.par2 "$dir/"
}

copy_par1_fixture_set() {
  local dir="$1"
  mkdir -p "$dir"
  cp "$ROOT/tests/fixtures/par1/flatdata/"* "$dir/"
}

make_single_source_fixture() {
  local dir="$1"
  mkdir -p "$dir"
  printf 'single source fixture for turbo parity\n' >"$dir/source.txt"
}

make_tree_source_fixture() {
  local dir="$1"
  mkdir -p "$dir/tree/nested"
  printf 'root source fixture\n' >"$dir/tree/root.txt"
  printf 'nested source fixture\n' >"$dir/tree/nested/child.txt"
}

pair_dirs() {
  local name="$1"
  TURBO_CASE="$TURBO_OUT/$name"
  PAR2RS_CASE="$PAR2RS_OUT/$name"
  mkdir -p "$TURBO_CASE" "$PAR2RS_CASE"
}

copy_fixture_pair() {
  pair_dirs "$1"
  copy_fixture_set "$TURBO_CASE"
  copy_fixture_set "$PAR2RS_CASE"
}

copy_par1_fixture_pair() {
  pair_dirs "$1"
  copy_par1_fixture_set "$TURBO_CASE"
  copy_par1_fixture_set "$PAR2RS_CASE"
}

make_source_pair() {
  pair_dirs "$1"
  make_single_source_fixture "$TURBO_CASE"
  make_single_source_fixture "$PAR2RS_CASE"
}

make_tree_pair() {
  pair_dirs "$1"
  make_tree_source_fixture "$TURBO_CASE"
  make_tree_source_fixture "$PAR2RS_CASE"
}

corrupt_pair_file() {
  local relative_path="$1"
  if [[ "$HAS_TURBO" = 1 ]]; then
    dd if=/dev/zero of="$TURBO_CASE/$relative_path" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null
  fi
  dd if=/dev/zero of="$PAR2RS_CASE/$relative_path" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null
}

run_pair() {
  local name="$1"
  shift
  TURBO_RESULT="$WORK_DIR/turbo-$name"
  PAR2RS_RESULT="$WORK_DIR/par2rs-$name"
  if [[ "$HAS_TURBO" = 1 ]]; then
    run_capture "$TURBO_CASE" "$TURBO_RESULT" "$TURBO_PAR2_CMD" "$@"
  fi
  run_capture "$PAR2RS_CASE" "$PAR2RS_RESULT" "$PAR2RS_BIN_DIR/par2" "$@"
}

run_pair_in_dirs() {
  local name="$1"
  local turbo_dir="$2"
  local par2rs_dir="$3"
  shift 3
  TURBO_RESULT="$WORK_DIR/turbo-$name"
  PAR2RS_RESULT="$WORK_DIR/par2rs-$name"
  if [[ "$HAS_TURBO" = 1 ]]; then
    run_capture "$turbo_dir" "$TURBO_RESULT" "$TURBO_PAR2_CMD" "$@"
  fi
  run_capture "$par2rs_dir" "$PAR2RS_RESULT" "$PAR2RS_BIN_DIR/par2" "$@"
}

run_standalone_pair() {
  local name="$1"
  local turbo_tool="$2"
  local par2rs_tool="$3"
  shift 3
  TURBO_RESULT="$WORK_DIR/turbo-$name"
  PAR2RS_RESULT="$WORK_DIR/par2rs-$name"
  if [[ "$HAS_TURBO" = 1 ]] && command -v "$turbo_tool" >/dev/null 2>&1; then
    run_capture "$TURBO_CASE" "$TURBO_RESULT" "$turbo_tool" "$@"
  fi
  run_capture "$PAR2RS_CASE" "$PAR2RS_RESULT" "$PAR2RS_BIN_DIR/$par2rs_tool" "$@"
}

verify_created_pair() {
  local par2_file="$1"
  shift
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE" "$par2_file" "$@"
  fi
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE" "$par2_file" "$@"
}

assert_create_pair_success() {
  local expected_par2="$1"
  local verify_par2="$2"
  shift 2
  assert_pair_same_status
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_par2_set_created "$TURBO_CASE" "$expected_par2"
  fi
  assert_par2_set_created "$PAR2RS_CASE" "$expected_par2"
  verify_created_pair "$verify_par2" "$@"
}

run_invalid_create_case() {
  local label="$1"
  shift
  make_source_pair "create-invalid-$label"
  run_pair "create-invalid-$label" create "$@" out.par2 source.txt
  assert_pair_nonzero_status
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_no_par2_recovery_files "$TURBO_CASE" out
  fi
  assert_no_par2_recovery_files "$PAR2RS_CASE" out
}

run_invalid_verify_repair_case() {
  local label="$1"
  shift
  copy_fixture_pair "invalid-verify-$label"
  run_pair "invalid-verify-$label" verify "$@" testfile.par2
  assert_pair_nonzero_status

  copy_fixture_pair "invalid-repair-$label"
  run_pair "invalid-repair-$label" repair "$@" testfile.par2
  assert_pair_nonzero_status
}

printf 'building par2rs release binaries\n'
(cd "$ROOT" && cargo build --release --bins >/dev/null)

mkdir -p "$WORK_DIR"
TURBO_OUT="$WORK_DIR/turbo"
PAR2RS_OUT="$WORK_DIR/par2rs"
mkdir -p "$TURBO_OUT" "$PAR2RS_OUT"

if [[ "$HAS_TURBO" != 1 ]]; then
  printf 'skipping turbo comparisons: %s is not executable or on PATH\n' "$TURBO_PAR2_CMD"
fi

case_create_basic() {
  make_source_pair create-basic
  run_pair create-basic create out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_alias() {
  make_source_pair create-alias
  run_pair create-alias c out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_top_level_version_flags() {
  pair_dirs version-flags
  run_pair version-short -V
  assert_pair_zero_status
  run_pair version-long -VV
  assert_pair_zero_status
}

case_create_standalone_wrapper() {
  make_source_pair create-standalone
  run_standalone_pair create-standalone "$TURBO_PAR2CREATE_CMD" par2create out.par2 source.txt
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_par2_set_created "$TURBO_CASE" out.par2
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE" out.par2
  fi
  assert_par2_set_created "$PAR2RS_CASE" out.par2
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE" out.par2
}

case_standalone_create_archive_name() {
  make_source_pair par2create-archive-name
  run_standalone_pair par2create-archive-name "$TURBO_PAR2CREATE_CMD" par2create -amain.par2 out.par2 source.txt
  assert_pair_same_status
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_par2_set_created "$TURBO_CASE" main.par2
    assert_absent "$TURBO_CASE/out.par2"
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE" main.par2
  fi
  assert_par2_set_created "$PAR2RS_CASE" main.par2
  assert_absent "$PAR2RS_CASE/out.par2"
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE" main.par2
}

case_standalone_create_basepath() {
  pair_dirs par2create-basepath
  mkdir -p "$TURBO_CASE/base/data" "$TURBO_CASE/work" "$PAR2RS_CASE/base/data" "$PAR2RS_CASE/work"
  printf 'standalone base path source fixture\n' >"$TURBO_CASE/base/data/source.txt"
  printf 'standalone base path source fixture\n' >"$PAR2RS_CASE/base/data/source.txt"
  TURBO_RESULT="$WORK_DIR/turbo-par2create-basepath"
  PAR2RS_RESULT="$WORK_DIR/par2rs-par2create-basepath"
  if [[ "$HAS_TURBO" = 1 ]] && command -v "$TURBO_PAR2CREATE_CMD" >/dev/null 2>&1; then
    run_capture "$TURBO_CASE/work" "$TURBO_RESULT" "$TURBO_PAR2CREATE_CMD" -B../base out.par2 ../base/data/source.txt
  fi
  run_capture "$PAR2RS_CASE/work" "$PAR2RS_RESULT" "$PAR2RS_BIN_DIR/par2create" -B../base out.par2 ../base/data/source.txt
  assert_pair_same_status
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_par2_set_created "$TURBO_CASE/work" out.par2
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE/work" out.par2 -B../base
  fi
  assert_par2_set_created "$PAR2RS_CASE/work" out.par2
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE/work" out.par2 -B../base
}

case_standalone_create_terminator_hyphen_file() {
  pair_dirs par2create-terminator
  printf 'standalone dash source fixture\n' >"$TURBO_CASE/-dash.txt"
  printf 'standalone dash source fixture\n' >"$PAR2RS_CASE/-dash.txt"
  run_standalone_pair par2create-terminator "$TURBO_PAR2CREATE_CMD" par2create out.par2 -- -dash.txt
  assert_pair_same_status
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_par2_set_created "$TURBO_CASE" out.par2
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE" out.par2
  fi
  assert_par2_set_created "$PAR2RS_CASE" out.par2
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE" out.par2
}

case_create_archive_name() {
  make_source_pair create-archive-name
  run_pair create-archive-name create -amain.par2 out.par2 source.txt
  assert_create_pair_success main.par2 main.par2
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_absent "$TURBO_CASE/out.par2"
  fi
  assert_absent "$PAR2RS_CASE/out.par2"
}

case_create_basepath() {
  pair_dirs create-basepath
  mkdir -p "$TURBO_CASE/base/data" "$TURBO_CASE/work" "$PAR2RS_CASE/base/data" "$PAR2RS_CASE/work"
  printf 'base path source fixture\n' >"$TURBO_CASE/base/data/source.txt"
  printf 'base path source fixture\n' >"$PAR2RS_CASE/base/data/source.txt"
  run_pair_in_dirs create-basepath "$TURBO_CASE/work" "$PAR2RS_CASE/work" create -B../base out.par2 ../base/data/source.txt
  assert_pair_same_status
  assert_pair_zero_status
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_par2_set_created "$TURBO_CASE/work" out.par2
    assert_verify_success_for_created_set "$TURBO_PAR2_CMD" "$TURBO_CASE/work" out.par2 -B../base
  fi
  assert_par2_set_created "$PAR2RS_CASE/work" out.par2
  assert_verify_success_for_created_set "$PAR2RS_BIN_DIR/par2" "$PAR2RS_CASE/work" out.par2 -B../base
}

case_create_recursive() {
  make_tree_pair create-recursive
  run_pair create-recursive create -R -Btree out.par2 tree
  assert_create_pair_success out.par2 out.par2 -Btree
  rm "$PAR2RS_CASE/tree/nested/child.txt"
  if [[ "$HAS_TURBO" = 1 ]]; then
    rm "$TURBO_CASE/tree/nested/child.txt"
    run_capture "$TURBO_CASE" "$WORK_DIR/turbo-create-recursive-missing" "$TURBO_PAR2_CMD" verify -Btree out.par2
    assert_nonzero_status "$WORK_DIR/turbo-create-recursive-missing"
  fi
  run_capture "$PAR2RS_CASE" "$WORK_DIR/par2rs-create-recursive-missing" "$PAR2RS_BIN_DIR/par2" verify -Btree out.par2
  assert_nonzero_status "$WORK_DIR/par2rs-create-recursive-missing"
}

case_create_terminator_hyphen_file() {
  pair_dirs create-terminator
  printf 'dash source fixture\n' >"$TURBO_CASE/-dash.txt"
  printf 'dash source fixture\n' >"$PAR2RS_CASE/-dash.txt"
  run_pair create-terminator create out.par2 -- -dash.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_block_count() {
  make_source_pair create-block-count
  run_pair create-block-count create -b8 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_block_size() {
  make_source_pair create-block-size
  run_pair create-block-size create -s4 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_redundancy_percent() {
  make_source_pair create-redundancy-percent
  run_pair create-redundancy-percent create -r10 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_redundancy_target_k() {
  make_source_pair create-redundancy-target-k
  run_pair create-redundancy-target-k create -rk1 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_redundancy_target_m() {
  make_source_pair create-redundancy-target-m
  run_pair create-redundancy-target-m create -rm1 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_recovery_block_count() {
  make_source_pair create-recovery-block-count
  run_pair create-recovery-block-count create -c2 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_first_recovery_block() {
  make_source_pair create-first-recovery-block
  run_pair create-first-recovery-block create -f3 -c2 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_uniform_recovery_files() {
  make_source_pair create-uniform
  run_pair create-uniform create -u -c3 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_limited_recovery_files() {
  make_source_pair create-limited
  run_pair create-limited create -l -c3 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_recovery_file_count() {
  make_source_pair create-file-count
  run_pair create-file-count create -n2 -c3 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_file_threads() {
  make_source_pair create-file-threads
  run_pair create-file-threads create -T1 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_threads() {
  make_source_pair create-threads
  run_pair create-threads create -t1 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_memory() {
  make_source_pair create-memory
  run_pair create-memory create -m1 out.par2 source.txt
  assert_create_pair_success out.par2 out.par2
}

case_create_invalid_options() {
  run_invalid_create_case b-and-s -b8 -s4
  run_invalid_create_case duplicate-b -b8 -b9
  run_invalid_create_case b-zero -b0
  run_invalid_create_case b-too-large -b32769
  run_invalid_create_case b-nonnumeric -babc
  run_invalid_create_case s-not-multiple -s5
  run_invalid_create_case r-and-c -r10 -c2
  run_invalid_create_case duplicate-r -r10 -r20
  run_invalid_create_case invalid-r-suffix -rx1
  run_invalid_create_case duplicate-c -c1 -c2
  run_invalid_create_case c-too-large -c32769
  run_invalid_create_case duplicate-f -f1 -f2 -c2
  run_invalid_create_case f-too-large -f32769 -c2
  run_invalid_create_case u-and-l -u -l
  run_invalid_create_case l-and-n -l -n2
  run_invalid_create_case duplicate-n -n2 -n3
  run_invalid_create_case n-too-large -n32
  run_invalid_create_case duplicate-m -m1 -m2
  run_invalid_create_case m-zero -m0
  run_invalid_create_case T-zero -T0
  run_invalid_create_case create-purge -p
  run_invalid_create_case create-rename-only -O
  run_invalid_create_case create-data-skipping -N
  run_invalid_create_case create-skip-leeway -S64
}

case_standalone_create_invalid_options() {
  make_source_pair par2create-invalid-duplicate-b
  run_standalone_pair par2create-invalid-duplicate-b "$TURBO_PAR2CREATE_CMD" par2create -b8 -b9 out.par2 source.txt
  assert_pair_nonzero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_no_par2_recovery_files "$TURBO_CASE" out
  fi
  assert_no_par2_recovery_files "$PAR2RS_CASE" out

  make_source_pair par2create-invalid-rename-only
  run_standalone_pair par2create-invalid-rename-only "$TURBO_PAR2CREATE_CMD" par2create -O out.par2 source.txt
  assert_pair_nonzero_status
  if [[ "$HAS_TURBO" = 1 && -e "$TURBO_RESULT.status" ]]; then
    assert_no_par2_recovery_files "$TURBO_CASE" out
  fi
  assert_no_par2_recovery_files "$PAR2RS_CASE" out
}

case_reject_create_overwrite() {
  make_source_pair create-overwrite
  printf keep >"$TURBO_CASE/out.par2"
  printf keep >"$PAR2RS_CASE/out.par2"
  run_pair create-overwrite create out.par2 source.txt
  assert_pair_same_status
  assert_pair_nonzero_status
  grep -qx keep "$PAR2RS_CASE/out.par2"
}

case_reject_create_volume_overwrite() {
  make_source_pair create-volume-overwrite
  printf keep >"$TURBO_CASE/out.vol0+1.par2"
  printf keep >"$PAR2RS_CASE/out.vol0+1.par2"
  run_pair create-volume-overwrite create -s4 -c1 out.par2 source.txt
  assert_pair_nonzero_status
  grep -qx keep "$PAR2RS_CASE/out.vol0+1.par2"
}

case_verify_intact_par2() {
  copy_fixture_pair par2-intact
  run_pair par2-intact verify testfile.par2
  assert_pair_same_status
}

case_repair_corrupted_par2_file() {
  copy_fixture_pair par2-repair-corrupt
  corrupt_pair_file testfile
  run_pair par2-repair-corrupt repair testfile.par2
  assert_pair_same_status
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_hash_equal "$TURBO_CASE/testfile" "$PAR2RS_CASE/testfile"
  fi
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_verify_repair_aliases() {
  copy_fixture_pair par2-verify-alias
  run_pair par2-verify-alias v testfile.par2
  assert_pair_same_status

  copy_fixture_pair par2-repair-alias
  corrupt_pair_file testfile
  run_pair par2-repair-alias r testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_standalone_verify_repair_wrappers() {
  copy_fixture_pair par2verify-standalone
  run_standalone_pair par2verify-standalone "$TURBO_PAR2VERIFY_CMD" par2verify testfile.par2
  assert_pair_same_status

  copy_fixture_pair par2repair-standalone
  corrupt_pair_file testfile
  run_standalone_pair par2repair-standalone "$TURBO_PAR2REPAIR_CMD" par2repair testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_report_unrepairable_missing_par2_file() {
  copy_fixture_pair par2-unrepairable-missing
  rm "$TURBO_CASE/testfile" "$PAR2RS_CASE/testfile"
  run_pair par2-unrepairable-missing repair testfile.par2
  assert_pair_same_status
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_same_file_presence "$TURBO_CASE/testfile" "$PAR2RS_CASE/testfile"
    if [[ -e "$TURBO_CASE/testfile" ]]; then
      assert_hash_equal "$TURBO_CASE/testfile" "$PAR2RS_CASE/testfile"
    fi
  fi
}

case_verify_by_data_file_input() {
  copy_fixture_pair par2-verify-data-input
  run_pair par2-verify-data-input verify testfile
  assert_pair_same_status
}

case_verify_by_volume_input() {
  copy_fixture_pair par2-verify-volume-input
  run_pair par2-verify-volume-input verify testfile.vol00+01.par2
  assert_pair_same_status
}

case_verify_uppercase_par2_main_input() {
  copy_fixture_pair par2-verify-uppercase-main
  mv "$TURBO_CASE/testfile.par2" "$TURBO_CASE/testfile.PAR2"
  mv "$PAR2RS_CASE/testfile.par2" "$PAR2RS_CASE/testfile.PAR2"
  run_pair par2-verify-uppercase-main verify testfile.PAR2
  assert_pair_same_status
}

case_verify_uppercase_par2_volume_input() {
  copy_fixture_pair par2-verify-uppercase-volume
  mv "$TURBO_CASE/testfile.vol00+01.par2" "$TURBO_CASE/testfile.vol00+01.PAR2"
  mv "$PAR2RS_CASE/testfile.vol00+01.par2" "$PAR2RS_CASE/testfile.vol00+01.PAR2"
  run_pair par2-verify-uppercase-volume verify testfile.vol00+01.PAR2
  assert_pair_same_status
}

case_repair_by_data_file_input() {
  copy_fixture_pair par2-repair-data-input
  corrupt_pair_file testfile
  run_pair par2-repair-data-input repair testfile
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_repair_by_volume_input() {
  copy_fixture_pair par2-repair-volume-input
  corrupt_pair_file testfile
  run_pair par2-repair-volume-input repair testfile.vol00+01.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_repair_renamed_par2_file_from_volume_input() {
  copy_fixture_pair par2-repair-renamed-volume
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-repair-renamed-volume repair testfile.vol00+01.par2 wrong-name.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
}

case_verify_with_basepath() {
  pair_dirs par2-verify-basepath
  mkdir -p "$TURBO_CASE/base" "$TURBO_CASE/work" "$PAR2RS_CASE/base" "$PAR2RS_CASE/work"
  cp "$ROOT/tests/fixtures/testfile" "$TURBO_CASE/base/"
  cp "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/base/"
  cp "$ROOT/tests/fixtures/testfile"*.par2 "$TURBO_CASE/work/"
  cp "$ROOT/tests/fixtures/testfile"*.par2 "$PAR2RS_CASE/work/"
  run_pair_in_dirs par2-verify-basepath "$TURBO_CASE/work" "$PAR2RS_CASE/work" verify -B../base testfile.par2
  assert_pair_same_status
}

case_repair_with_basepath() {
  pair_dirs par2-repair-basepath
  mkdir -p "$TURBO_CASE/base" "$TURBO_CASE/work" "$PAR2RS_CASE/base" "$PAR2RS_CASE/work"
  cp "$ROOT/tests/fixtures/testfile" "$TURBO_CASE/base/"
  cp "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/base/"
  cp "$ROOT/tests/fixtures/testfile"*.par2 "$TURBO_CASE/work/"
  cp "$ROOT/tests/fixtures/testfile"*.par2 "$PAR2RS_CASE/work/"
  corrupt_pair_file base/testfile
  run_pair_in_dirs par2-repair-basepath "$TURBO_CASE/work" "$PAR2RS_CASE/work" repair -B../base testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/base/testfile"
}

case_verify_with_data_skipping() {
  copy_fixture_pair par2-verify-N
  run_pair par2-verify-N verify -N testfile.par2
  assert_pair_same_status
}

case_verify_with_data_skipping_leeway() {
  copy_fixture_pair par2-verify-NS
  run_pair par2-verify-NS verify -N -S64 testfile.par2
  assert_pair_same_status
}

case_repair_with_data_skipping_leeway() {
  copy_fixture_pair par2-repair-NS
  corrupt_pair_file testfile
  run_pair par2-repair-NS repair -N -S64 testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_verify_with_file_threads() {
  copy_fixture_pair par2-verify-T
  run_pair par2-verify-T verify -T1 testfile.par2
  assert_pair_same_status
}

case_repair_with_file_threads() {
  copy_fixture_pair par2-repair-T
  corrupt_pair_file testfile
  run_pair par2-repair-T repair -T1 testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_verify_with_memory() {
  copy_fixture_pair par2-verify-memory
  run_pair par2-verify-memory verify -m1 testfile.par2
  assert_pair_same_status
}

case_repair_with_memory() {
  copy_fixture_pair par2-repair-memory
  corrupt_pair_file testfile
  run_pair par2-repair-memory repair -m1 testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
}

case_verify_repair_hyphen_extra() {
  copy_fixture_pair par2-hyphen-extra-verify
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/-renamed.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/-renamed.bin"
  run_pair par2-hyphen-extra-verify verify testfile.par2 -- -renamed.bin
  assert_pair_same_status
  assert_file_exists "$PAR2RS_CASE/-renamed.bin"
  assert_absent "$PAR2RS_CASE/testfile"

  copy_fixture_pair par2-hyphen-extra-repair
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/-renamed.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/-renamed.bin"
  run_pair par2-hyphen-extra-repair repair testfile.par2 -- -renamed.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/-renamed.bin"
}

case_standalone_verify_repair_hyphen_extra() {
  copy_fixture_pair par2verify-hyphen-extra
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/-renamed.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/-renamed.bin"
  run_standalone_pair par2verify-hyphen-extra "$TURBO_PAR2VERIFY_CMD" par2verify testfile.par2 -- -renamed.bin
  assert_pair_same_status
  assert_file_exists "$PAR2RS_CASE/-renamed.bin"
  assert_absent "$PAR2RS_CASE/testfile"

  copy_fixture_pair par2repair-hyphen-extra
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/-renamed.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/-renamed.bin"
  run_standalone_pair par2repair-hyphen-extra "$TURBO_PAR2REPAIR_CMD" par2repair testfile.par2 -- -renamed.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/-renamed.bin"
}

case_repair_renamed_par2_file() {
  copy_fixture_pair par2-repair-renamed
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-repair-renamed repair testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
}

case_repair_renamed_par2_file_rename_only() {
  copy_fixture_pair par2-repair-renamed-O
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-repair-renamed-O repair -O testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
}

case_standalone_repair_renamed_par2_file_rename_only() {
  copy_fixture_pair par2repair-renamed-O
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_standalone_pair par2repair-renamed-O "$TURBO_PAR2REPAIR_CMD" par2repair -O testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
}

case_verify_renamed_par2_file_rename_only() {
  copy_fixture_pair par2-verify-renamed-O
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-verify-renamed-O verify -O testfile.par2 wrong-name.bin
  assert_pair_same_status
}

case_repair_damaged_renamed_par2_file_rename_only() {
  copy_fixture_pair par2-repair-damaged-renamed-O
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  printf damaged >"$TURBO_CASE/wrong-name.bin"
  printf damaged >"$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-repair-damaged-renamed-O repair -O testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_pair_nonzero_status
  assert_absent "$PAR2RS_CASE/testfile"
  assert_file_exists "$PAR2RS_CASE/wrong-name.bin"
}

case_par2_purge_after_intact_verify() {
  copy_fixture_pair par2-purge-verify
  run_pair par2-purge-verify verify -p testfile.par2
  assert_pair_same_status
  assert_file_exists "$PAR2RS_CASE/testfile"
  assert_no_par2_recovery_files "$PAR2RS_CASE" testfile
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_file_exists "$TURBO_CASE/testfile"
    assert_no_par2_recovery_files "$TURBO_CASE" testfile
  fi
}

case_par2_purge_after_repair() {
  copy_fixture_pair par2-purge-repair
  corrupt_pair_file testfile
  run_pair par2-purge-repair repair -p testfile.par2
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_no_par2_recovery_files "$PAR2RS_CASE" testfile
}

case_par2_purge_after_rename_repair() {
  copy_fixture_pair par2-purge-rename-repair
  mv "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  mv "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  run_pair par2-purge-rename-repair repair -p testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_pair_zero_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
  assert_no_par2_recovery_files "$PAR2RS_CASE" testfile
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_hash_equal "$ROOT/tests/fixtures/testfile" "$TURBO_CASE/testfile"
    assert_absent "$TURBO_CASE/wrong-name.bin"
    assert_no_par2_recovery_files "$TURBO_CASE" testfile
  fi
}

case_par2_purge_after_rename_backup_repair() {
  copy_fixture_pair par2-purge-rename-backup-repair
  cp "$TURBO_CASE/testfile" "$TURBO_CASE/wrong-name.bin"
  cp "$PAR2RS_CASE/testfile" "$PAR2RS_CASE/wrong-name.bin"
  corrupt_pair_file testfile
  run_pair par2-purge-rename-backup-repair repair -p testfile.par2 wrong-name.bin
  assert_pair_same_status
  assert_pair_zero_status
  assert_hash_equal "$ROOT/tests/fixtures/testfile" "$PAR2RS_CASE/testfile"
  assert_absent "$PAR2RS_CASE/wrong-name.bin"
  assert_absent "$PAR2RS_CASE/testfile.1"
  assert_no_par2_recovery_files "$PAR2RS_CASE" testfile
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_hash_equal "$ROOT/tests/fixtures/testfile" "$TURBO_CASE/testfile"
    assert_absent "$TURBO_CASE/wrong-name.bin"
    assert_absent "$TURBO_CASE/testfile.1"
    assert_no_par2_recovery_files "$TURBO_CASE" testfile
  fi
}

case_failed_par2_repair_with_purge_keeps_recovery() {
  copy_fixture_pair par2-purge-failed
  rm "$TURBO_CASE/testfile" "$PAR2RS_CASE/testfile"
  run_pair par2-purge-failed repair -p testfile.par2
  assert_pair_nonzero_status
  assert_file_exists "$PAR2RS_CASE/testfile.par2"
  assert_glob_present "$PAR2RS_CASE/testfile.vol*.par2"
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_file_exists "$TURBO_CASE/testfile.par2"
    assert_glob_present "$TURBO_CASE/testfile.vol*.par2"
  fi
}

case_verify_repair_invalid_options() {
  run_invalid_verify_repair_case S-without-N -S64
  run_invalid_verify_repair_case S-zero -N -S0
  run_invalid_verify_repair_case R-create-only -R
  run_invalid_verify_repair_case b-create-only -b8
  run_invalid_verify_repair_case s-create-only -s4
  run_invalid_verify_repair_case r-create-only -r10
  run_invalid_verify_repair_case c-create-only -c2
  run_invalid_verify_repair_case f-create-only -f1
  run_invalid_verify_repair_case u-create-only -u
  run_invalid_verify_repair_case l-create-only -l
  run_invalid_verify_repair_case n-create-only -n2
  run_invalid_verify_repair_case T-zero -T0
  run_invalid_verify_repair_case m-zero -m0
}

case_standalone_verify_repair_invalid_options() {
  copy_fixture_pair par2verify-invalid-S
  run_standalone_pair par2verify-invalid-S "$TURBO_PAR2VERIFY_CMD" par2verify -S64 testfile.par2
  assert_pair_nonzero_status

  copy_fixture_pair par2repair-invalid-S
  run_standalone_pair par2repair-invalid-S "$TURBO_PAR2REPAIR_CMD" par2repair -S64 testfile.par2
  assert_pair_nonzero_status
}

case_verify_intact_par1() {
  copy_par1_fixture_pair par1-intact
  run_pair par1-intact verify testdata.par
  assert_pair_same_status
}

case_verify_par1_from_volume_input() {
  copy_par1_fixture_pair par1-volume-verify
  run_pair par1-volume-verify verify testdata.p01
  assert_pair_same_status
}

case_verify_par1_uppercase_main() {
  copy_par1_fixture_pair par1-uppercase-main
  mv "$TURBO_CASE/testdata.par" "$TURBO_CASE/testdata.PAR"
  mv "$PAR2RS_CASE/testdata.par" "$PAR2RS_CASE/testdata.PAR"
  run_pair par1-uppercase-main verify testdata.PAR
  assert_pair_same_status
}

case_verify_par1_uppercase_volume() {
  copy_par1_fixture_pair par1-uppercase-volume
  mv "$TURBO_CASE/testdata.p01" "$TURBO_CASE/testdata.P01"
  mv "$PAR2RS_CASE/testdata.p01" "$PAR2RS_CASE/testdata.P01"
  run_pair par1-uppercase-volume verify testdata.P01
  assert_pair_same_status
}

case_repair_missing_par1_file() {
  copy_par1_fixture_pair par1-repair-missing
  rm "$TURBO_CASE/test-3.data" "$PAR2RS_CASE/test-3.data"
  run_pair par1-repair-missing repair testdata.par
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_CASE/test-3.data"
}

case_repair_par1_from_volume_input() {
  copy_par1_fixture_pair par1-repair-volume
  rm "$TURBO_CASE/test-3.data" "$PAR2RS_CASE/test-3.data"
  run_pair par1-repair-volume repair testdata.p01
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_CASE/test-3.data"
}

case_repair_renamed_par1_file() {
  copy_par1_fixture_pair par1-repair-renamed
  mv "$TURBO_CASE/test-4.data" "$TURBO_CASE/wrong-name.data"
  mv "$PAR2RS_CASE/test-4.data" "$PAR2RS_CASE/wrong-name.data"
  run_pair par1-repair-renamed repair testdata.par wrong-name.data
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-4.data" "$PAR2RS_CASE/test-4.data"
  assert_absent "$PAR2RS_CASE/wrong-name.data"
}

case_repair_renamed_par1_from_volume_input() {
  copy_par1_fixture_pair par1-repair-renamed-volume
  mv "$TURBO_CASE/test-4.data" "$TURBO_CASE/wrong-name.data"
  mv "$PAR2RS_CASE/test-4.data" "$PAR2RS_CASE/wrong-name.data"
  run_pair par1-repair-renamed-volume repair testdata.p01 wrong-name.data
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-4.data" "$PAR2RS_CASE/test-4.data"
  assert_absent "$PAR2RS_CASE/wrong-name.data"
}

case_purge_intact_par1() {
  copy_par1_fixture_pair par1-purge-intact
  run_pair par1-purge-intact verify -p testdata.par
  assert_pair_same_status
  assert_no_par1_recovery_files "$PAR2RS_CASE"
  if [[ "$HAS_TURBO" = 1 ]]; then
    assert_no_par1_recovery_files "$TURBO_CASE"
  fi
}

case_purge_after_par1_repair() {
  copy_par1_fixture_pair par1-purge-repair
  rm "$TURBO_CASE/test-3.data" "$PAR2RS_CASE/test-3.data"
  run_pair par1-purge-repair repair -p testdata.par
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_CASE/test-3.data"
  assert_no_par1_recovery_files "$PAR2RS_CASE"
}

case_failed_par1_repair_with_purge_keeps_recovery() {
  copy_par1_fixture_pair par1-purge-failed
  rm "$TURBO_CASE/test-0.data" "$TURBO_CASE/test-1.data" "$TURBO_CASE/test-2.data" "$TURBO_CASE/test-3.data"
  rm "$PAR2RS_CASE/test-0.data" "$PAR2RS_CASE/test-1.data" "$PAR2RS_CASE/test-2.data" "$PAR2RS_CASE/test-3.data"
  run_pair par1-purge-failed repair -p testdata.par
  assert_pair_nonzero_status
  assert_file_exists "$PAR2RS_CASE/testdata.par"
  assert_file_exists "$PAR2RS_CASE/testdata.p01"
  assert_file_exists "$PAR2RS_CASE/testdata.p02"
}

case_par1_rename_only_acceptance() {
  copy_par1_fixture_pair par1-verify-O
  run_pair par1-verify-O verify -O testdata.par
  assert_pair_same_status

  copy_par1_fixture_pair par1-repair-O
  run_pair par1-repair-O repair -O testdata.par
  assert_pair_same_status
}

case_standalone_par1_verify_repair() {
  copy_par1_fixture_pair par1-standalone-verify
  run_standalone_pair par1-standalone-verify "$TURBO_PAR2VERIFY_CMD" par2verify testdata.par
  assert_pair_same_status

  copy_par1_fixture_pair par1-standalone-repair
  rm "$TURBO_CASE/test-3.data" "$PAR2RS_CASE/test-3.data"
  run_standalone_pair par1-standalone-repair "$TURBO_PAR2REPAIR_CMD" par2repair testdata.par
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_CASE/test-3.data"
}

case_standalone_par1_repair_renamed() {
  copy_par1_fixture_pair par1-standalone-repair-renamed
  mv "$TURBO_CASE/test-4.data" "$TURBO_CASE/wrong-name.data"
  mv "$PAR2RS_CASE/test-4.data" "$PAR2RS_CASE/wrong-name.data"
  run_standalone_pair par1-standalone-repair-renamed "$TURBO_PAR2REPAIR_CMD" par2repair testdata.par wrong-name.data
  assert_pair_same_status
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-4.data" "$PAR2RS_CASE/test-4.data"
  assert_absent "$PAR2RS_CASE/wrong-name.data"
}

case_standalone_par1_rename_only_acceptance() {
  copy_par1_fixture_pair par1-standalone-verify-O
  run_standalone_pair par1-standalone-verify-O "$TURBO_PAR2VERIFY_CMD" par2verify -O testdata.par
  assert_pair_same_status

  copy_par1_fixture_pair par1-standalone-repair-O
  run_standalone_pair par1-standalone-repair-O "$TURBO_PAR2REPAIR_CMD" par2repair -O testdata.par
  assert_pair_same_status
}

case_reject_par1_create_self() {
  pair_dirs par1-create-reject
  printf source >"$PAR2RS_CASE/source.txt"
  PAR2RS_RESULT="$WORK_DIR/par2rs-par1-create-reject"
  run_capture "$PAR2RS_CASE" "$PAR2RS_RESULT" "$PAR2RS_BIN_DIR/par2" create out.par source.txt
  assert_nonzero_status "$PAR2RS_RESULT"
  assert_absent "$PAR2RS_CASE/out.par"
}

run_case "create basic PAR2" case_create_basic
run_case "create PAR2 with c alias" case_create_alias
run_case "top-level version flags" case_top_level_version_flags
run_case "create PAR2 with standalone wrapper" case_create_standalone_wrapper
run_case "standalone create PAR2 with -a archive name" case_standalone_create_archive_name
run_case "standalone create PAR2 with -B basepath" case_standalone_create_basepath
run_case "standalone create PAR2 with -- hyphen filename" case_standalone_create_terminator_hyphen_file
run_case "create PAR2 with -a archive name" case_create_archive_name
run_case "create PAR2 with -B basepath" case_create_basepath
run_case "create PAR2 recursively" case_create_recursive
run_case "create PAR2 with -- hyphen filename" case_create_terminator_hyphen_file
run_case "create PAR2 with -b" case_create_block_count
run_case "create PAR2 with -s" case_create_block_size
run_case "create PAR2 with -r percent" case_create_redundancy_percent
run_case "create PAR2 with -rk target" case_create_redundancy_target_k
run_case "create PAR2 with -rm target" case_create_redundancy_target_m
run_case "create PAR2 with -c" case_create_recovery_block_count
run_case "create PAR2 with -f" case_create_first_recovery_block
run_case "create PAR2 with -u" case_create_uniform_recovery_files
run_case "create PAR2 with -l" case_create_limited_recovery_files
run_case "create PAR2 with -n" case_create_recovery_file_count
run_case "create PAR2 with -T" case_create_file_threads
run_case "create PAR2 with -t" case_create_threads
run_case "create PAR2 with -m" case_create_memory
run_case "reject invalid PAR2 create options" case_create_invalid_options
run_case "reject invalid standalone PAR2 create options" case_standalone_create_invalid_options
run_case "reject create overwrite" case_reject_create_overwrite
run_case "reject create volume overwrite" case_reject_create_volume_overwrite
run_case "verify intact PAR2" case_verify_intact_par2
run_case "repair corrupted PAR2 file" case_repair_corrupted_par2_file
run_case "verify and repair PAR2 with v/r aliases" case_verify_repair_aliases
run_case "verify and repair PAR2 with standalone wrappers" case_standalone_verify_repair_wrappers
run_case "report unrepairable missing PAR2 file" case_report_unrepairable_missing_par2_file
run_case "verify PAR2 by data file input" case_verify_by_data_file_input
run_case "verify PAR2 by volume input" case_verify_by_volume_input
run_case "verify PAR2 uppercase main input" case_verify_uppercase_par2_main_input
run_case "verify PAR2 uppercase volume input" case_verify_uppercase_par2_volume_input
run_case "repair PAR2 by data file input" case_repair_by_data_file_input
run_case "repair PAR2 by volume input" case_repair_by_volume_input
run_case "repair renamed PAR2 file from volume input" case_repair_renamed_par2_file_from_volume_input
run_case "verify PAR2 with -B" case_verify_with_basepath
run_case "repair PAR2 with -B" case_repair_with_basepath
run_case "verify PAR2 with -N" case_verify_with_data_skipping
run_case "verify PAR2 with -N -S" case_verify_with_data_skipping_leeway
run_case "repair PAR2 with -N -S" case_repair_with_data_skipping_leeway
run_case "verify PAR2 with -T" case_verify_with_file_threads
run_case "repair PAR2 with -T" case_repair_with_file_threads
run_case "verify PAR2 with -m" case_verify_with_memory
run_case "repair PAR2 with -m" case_repair_with_memory
run_case "verify and repair PAR2 with -- hyphen extra" case_verify_repair_hyphen_extra
run_case "standalone verify and repair PAR2 with -- hyphen extra" case_standalone_verify_repair_hyphen_extra
run_case "repair renamed PAR2 file" case_repair_renamed_par2_file
run_case "repair renamed PAR2 file with -O" case_repair_renamed_par2_file_rename_only
run_case "standalone repair renamed PAR2 file with -O" case_standalone_repair_renamed_par2_file_rename_only
run_case "verify renamed PAR2 file with -O" case_verify_renamed_par2_file_rename_only
run_case "repair damaged renamed PAR2 file with -O" case_repair_damaged_renamed_par2_file_rename_only
run_case "purge intact PAR2" case_par2_purge_after_intact_verify
run_case "purge repaired PAR2" case_par2_purge_after_repair
run_case "purge PAR2 after repair by rename" case_par2_purge_after_rename_repair
run_case "purge PAR2 after repair by rename with backup" case_par2_purge_after_rename_backup_repair
run_case "failed PAR2 repair with purge keeps recovery files" case_failed_par2_repair_with_purge_keeps_recovery
run_case "reject invalid PAR2 verify/repair options" case_verify_repair_invalid_options
run_case "reject invalid standalone PAR2 verify/repair options" case_standalone_verify_repair_invalid_options
run_case "verify intact PAR1" case_verify_intact_par1
run_case "verify PAR1 from volume input" case_verify_par1_from_volume_input
run_case "verify PAR1 uppercase main input" case_verify_par1_uppercase_main
run_case "verify PAR1 uppercase volume input" case_verify_par1_uppercase_volume
run_case "repair missing PAR1 file" case_repair_missing_par1_file
run_case "repair PAR1 from volume input" case_repair_par1_from_volume_input
run_case "repair renamed PAR1 file" case_repair_renamed_par1_file
run_case "repair renamed PAR1 file from volume input" case_repair_renamed_par1_from_volume_input
run_case "purge intact PAR1" case_purge_intact_par1
run_case "purge repaired PAR1" case_purge_after_par1_repair
run_case "failed PAR1 repair with purge keeps recovery files" case_failed_par1_repair_with_purge_keeps_recovery
run_case "PAR1 accepts -O" case_par1_rename_only_acceptance
run_case "standalone PAR1 verify and repair" case_standalone_par1_verify_repair
run_case "standalone PAR1 repair renamed file" case_standalone_par1_repair_renamed
run_case "standalone PAR1 accepts -O" case_standalone_par1_rename_only_acceptance
run_case "reject PAR1 create" case_reject_par1_create_self

printf 'comparison work dir: %s\n' "$WORK_DIR"

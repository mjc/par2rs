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
if command -v "$TURBO_PAR2_CMD" >/dev/null 2>&1; then
  HAS_TURBO=1
else
  HAS_TURBO=0
fi

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

printf 'building par2rs release binaries\n'
(cd "$ROOT" && cargo build --release --bins >/dev/null)

mkdir -p "$WORK_DIR"
TURBO_OUT="$WORK_DIR/turbo"
PAR2RS_OUT="$WORK_DIR/par2rs"
mkdir -p "$TURBO_OUT" "$PAR2RS_OUT"

case_verify_intact_par2() {
  copy_fixture_set "$TURBO_OUT/par2-intact"
  copy_fixture_set "$PAR2RS_OUT/par2-intact"
  run_capture "$TURBO_OUT/par2-intact" "$WORK_DIR/turbo-par2-intact" "$TURBO_PAR2_CMD" verify testfile.par2
  run_capture "$PAR2RS_OUT/par2-intact" "$WORK_DIR/par2rs-par2-intact" "$PAR2RS_BIN_DIR/par2" verify testfile.par2
  assert_same_status "$WORK_DIR/turbo-par2-intact" "$WORK_DIR/par2rs-par2-intact"
}

case_repair_corrupted_par2_file() {
  copy_fixture_set "$TURBO_OUT/par2-repair-corrupt"
  copy_fixture_set "$PAR2RS_OUT/par2-repair-corrupt"
  dd if=/dev/zero of="$TURBO_OUT/par2-repair-corrupt/testfile" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null
  dd if=/dev/zero of="$PAR2RS_OUT/par2-repair-corrupt/testfile" bs=1 count=100 seek=1000 conv=notrunc 2>/dev/null
  run_capture "$TURBO_OUT/par2-repair-corrupt" "$WORK_DIR/turbo-par2-repair-corrupt" "$TURBO_PAR2_CMD" repair testfile.par2
  run_capture "$PAR2RS_OUT/par2-repair-corrupt" "$WORK_DIR/par2rs-par2-repair-corrupt" "$PAR2RS_BIN_DIR/par2" repair testfile.par2
  assert_same_status "$WORK_DIR/turbo-par2-repair-corrupt" "$WORK_DIR/par2rs-par2-repair-corrupt"
  assert_hash_equal "$TURBO_OUT/par2-repair-corrupt/testfile" "$PAR2RS_OUT/par2-repair-corrupt/testfile"
}

case_report_unrepairable_missing_par2_file() {
  copy_fixture_set "$TURBO_OUT/par2-unrepairable-missing"
  copy_fixture_set "$PAR2RS_OUT/par2-unrepairable-missing"
  rm "$TURBO_OUT/par2-unrepairable-missing/testfile" "$PAR2RS_OUT/par2-unrepairable-missing/testfile"
  run_capture "$TURBO_OUT/par2-unrepairable-missing" "$WORK_DIR/turbo-par2-unrepairable-missing" "$TURBO_PAR2_CMD" repair testfile.par2
  run_capture "$PAR2RS_OUT/par2-unrepairable-missing" "$WORK_DIR/par2rs-par2-unrepairable-missing" "$PAR2RS_BIN_DIR/par2" repair testfile.par2
  assert_same_status "$WORK_DIR/turbo-par2-unrepairable-missing" "$WORK_DIR/par2rs-par2-unrepairable-missing"
  assert_same_file_presence "$TURBO_OUT/par2-unrepairable-missing/testfile" "$PAR2RS_OUT/par2-unrepairable-missing/testfile"
  if [[ -e "$TURBO_OUT/par2-unrepairable-missing/testfile" ]]; then
    assert_hash_equal "$TURBO_OUT/par2-unrepairable-missing/testfile" "$PAR2RS_OUT/par2-unrepairable-missing/testfile"
  fi
}

case_reject_create_overwrite() {
  mkdir -p "$TURBO_OUT/create-overwrite" "$PAR2RS_OUT/create-overwrite"
  printf source >"$TURBO_OUT/create-overwrite/source.txt"
  printf source >"$PAR2RS_OUT/create-overwrite/source.txt"
  printf keep >"$TURBO_OUT/create-overwrite/out.par2"
  printf keep >"$PAR2RS_OUT/create-overwrite/out.par2"
  run_capture "$TURBO_OUT/create-overwrite" "$WORK_DIR/turbo-create-overwrite" "$TURBO_PAR2_CMD" create out.par2 source.txt
  run_capture "$PAR2RS_OUT/create-overwrite" "$WORK_DIR/par2rs-create-overwrite" "$PAR2RS_BIN_DIR/par2" create out.par2 source.txt
  assert_same_status "$WORK_DIR/turbo-create-overwrite" "$WORK_DIR/par2rs-create-overwrite"
  grep -qx keep "$PAR2RS_OUT/create-overwrite/out.par2"
}

case_verify_intact_par1() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-intact"
  run_capture "$PAR2RS_OUT/par1-intact" "$WORK_DIR/par2rs-par1-intact" "$PAR2RS_BIN_DIR/par2" verify testdata.par
  test "$(cat "$WORK_DIR/par2rs-par1-intact.status")" = "0"
}

if [[ "$HAS_TURBO" = 1 ]]; then
  run_case "verify intact PAR2" case_verify_intact_par2
  run_case "repair corrupted PAR2 file" case_repair_corrupted_par2_file
  run_case "report unrepairable missing PAR2 file" case_report_unrepairable_missing_par2_file
  run_case "reject create overwrite" case_reject_create_overwrite
else
  printf 'skipping turbo comparisons: %s is not executable or on PATH\n' "$TURBO_PAR2_CMD"
fi
run_case "verify intact PAR1" case_verify_intact_par1

printf 'comparison work dir: %s\n' "$WORK_DIR"

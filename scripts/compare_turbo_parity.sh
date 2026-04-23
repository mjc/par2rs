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

assert_absent() {
  local path="$1"
  if [[ -e "$path" ]]; then
    printf 'expected absent: %s\n' "$path" >&2
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

assert_no_par1_recovery_files() {
  local dir="$1"
  assert_absent "$dir/testdata.par"
  assert_absent "$dir/testdata.p01"
  assert_absent "$dir/testdata.p02"
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
  copy_par1_fixture_set "$TURBO_OUT/par1-intact"
  copy_par1_fixture_set "$PAR2RS_OUT/par1-intact"
  run_capture "$TURBO_OUT/par1-intact" "$WORK_DIR/turbo-par1-intact" "$TURBO_PAR2_CMD" verify testdata.par
  run_capture "$PAR2RS_OUT/par1-intact" "$WORK_DIR/par2rs-par1-intact" "$PAR2RS_BIN_DIR/par2" verify testdata.par
  assert_same_status "$WORK_DIR/turbo-par1-intact" "$WORK_DIR/par2rs-par1-intact"
}

case_verify_par1_from_volume_input() {
  copy_par1_fixture_set "$TURBO_OUT/par1-volume-verify"
  copy_par1_fixture_set "$PAR2RS_OUT/par1-volume-verify"
  run_capture "$TURBO_OUT/par1-volume-verify" "$WORK_DIR/turbo-par1-volume-verify" "$TURBO_PAR2_CMD" verify testdata.p01
  run_capture "$PAR2RS_OUT/par1-volume-verify" "$WORK_DIR/par2rs-par1-volume-verify" "$PAR2RS_BIN_DIR/par2" verify testdata.p01
  assert_same_status "$WORK_DIR/turbo-par1-volume-verify" "$WORK_DIR/par2rs-par1-volume-verify"
}

case_repair_missing_par1_file() {
  copy_par1_fixture_set "$TURBO_OUT/par1-repair-missing"
  copy_par1_fixture_set "$PAR2RS_OUT/par1-repair-missing"
  rm "$TURBO_OUT/par1-repair-missing/test-3.data" "$PAR2RS_OUT/par1-repair-missing/test-3.data"
  run_capture "$TURBO_OUT/par1-repair-missing" "$WORK_DIR/turbo-par1-repair-missing" "$TURBO_PAR2_CMD" repair testdata.par
  run_capture "$PAR2RS_OUT/par1-repair-missing" "$WORK_DIR/par2rs-par1-repair-missing" "$PAR2RS_BIN_DIR/par2" repair testdata.par
  assert_same_status "$WORK_DIR/turbo-par1-repair-missing" "$WORK_DIR/par2rs-par1-repair-missing"
  assert_hash_equal "$TURBO_OUT/par1-repair-missing/test-3.data" "$PAR2RS_OUT/par1-repair-missing/test-3.data"
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_OUT/par1-repair-missing/test-3.data"
}

case_repair_renamed_par1_file() {
  copy_par1_fixture_set "$TURBO_OUT/par1-repair-renamed"
  copy_par1_fixture_set "$PAR2RS_OUT/par1-repair-renamed"
  mv "$TURBO_OUT/par1-repair-renamed/test-4.data" "$TURBO_OUT/par1-repair-renamed/wrong-name.data"
  mv "$PAR2RS_OUT/par1-repair-renamed/test-4.data" "$PAR2RS_OUT/par1-repair-renamed/wrong-name.data"
  run_capture "$TURBO_OUT/par1-repair-renamed" "$WORK_DIR/turbo-par1-repair-renamed" "$TURBO_PAR2_CMD" repair testdata.par wrong-name.data
  run_capture "$PAR2RS_OUT/par1-repair-renamed" "$WORK_DIR/par2rs-par1-repair-renamed" "$PAR2RS_BIN_DIR/par2" repair testdata.par wrong-name.data
  assert_same_status "$WORK_DIR/turbo-par1-repair-renamed" "$WORK_DIR/par2rs-par1-repair-renamed"
  assert_hash_equal "$TURBO_OUT/par1-repair-renamed/test-4.data" "$PAR2RS_OUT/par1-repair-renamed/test-4.data"
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-4.data" "$PAR2RS_OUT/par1-repair-renamed/test-4.data"
  assert_same_file_presence "$TURBO_OUT/par1-repair-renamed/wrong-name.data" "$PAR2RS_OUT/par1-repair-renamed/wrong-name.data"
  assert_absent "$PAR2RS_OUT/par1-repair-renamed/wrong-name.data"
}

case_purge_intact_par1() {
  copy_par1_fixture_set "$TURBO_OUT/par1-purge-intact"
  copy_par1_fixture_set "$PAR2RS_OUT/par1-purge-intact"
  run_capture "$TURBO_OUT/par1-purge-intact" "$WORK_DIR/turbo-par1-purge-intact" "$TURBO_PAR2_CMD" verify -p testdata.par
  run_capture "$PAR2RS_OUT/par1-purge-intact" "$WORK_DIR/par2rs-par1-purge-intact" "$PAR2RS_BIN_DIR/par2" verify -p testdata.par
  assert_same_status "$WORK_DIR/turbo-par1-purge-intact" "$WORK_DIR/par2rs-par1-purge-intact"
  assert_no_par1_recovery_files "$TURBO_OUT/par1-purge-intact"
  assert_no_par1_recovery_files "$PAR2RS_OUT/par1-purge-intact"
}

case_verify_intact_par1_self() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-intact"
  run_capture "$PAR2RS_OUT/par1-intact" "$WORK_DIR/par2rs-par1-intact" "$PAR2RS_BIN_DIR/par2" verify testdata.par
  test "$(cat "$WORK_DIR/par2rs-par1-intact.status")" = "0"
}

case_verify_par1_from_volume_input_self() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-volume-verify"
  run_capture "$PAR2RS_OUT/par1-volume-verify" "$WORK_DIR/par2rs-par1-volume-verify" "$PAR2RS_BIN_DIR/par2" verify testdata.p01
  test "$(cat "$WORK_DIR/par2rs-par1-volume-verify.status")" = "0"
}

case_repair_missing_par1_file_self() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-repair-missing"
  rm "$PAR2RS_OUT/par1-repair-missing/test-3.data"
  run_capture "$PAR2RS_OUT/par1-repair-missing" "$WORK_DIR/par2rs-par1-repair-missing" "$PAR2RS_BIN_DIR/par2" repair testdata.par
  test "$(cat "$WORK_DIR/par2rs-par1-repair-missing.status")" = "0"
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-3.data" "$PAR2RS_OUT/par1-repair-missing/test-3.data"
}

case_repair_renamed_par1_file_self() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-repair-renamed"
  mv "$PAR2RS_OUT/par1-repair-renamed/test-4.data" "$PAR2RS_OUT/par1-repair-renamed/wrong-name.data"
  run_capture "$PAR2RS_OUT/par1-repair-renamed" "$WORK_DIR/par2rs-par1-repair-renamed" "$PAR2RS_BIN_DIR/par2" repair testdata.par wrong-name.data
  test "$(cat "$WORK_DIR/par2rs-par1-repair-renamed.status")" = "0"
  assert_hash_equal "$ROOT/tests/fixtures/par1/flatdata/test-4.data" "$PAR2RS_OUT/par1-repair-renamed/test-4.data"
  assert_absent "$PAR2RS_OUT/par1-repair-renamed/wrong-name.data"
}

case_purge_intact_par1_self() {
  copy_par1_fixture_set "$PAR2RS_OUT/par1-purge-intact"
  run_capture "$PAR2RS_OUT/par1-purge-intact" "$WORK_DIR/par2rs-par1-purge-intact" "$PAR2RS_BIN_DIR/par2" verify -p testdata.par
  test "$(cat "$WORK_DIR/par2rs-par1-purge-intact.status")" = "0"
  assert_no_par1_recovery_files "$PAR2RS_OUT/par1-purge-intact"
}

case_reject_par1_create_self() {
  mkdir -p "$PAR2RS_OUT/par1-create-reject"
  printf source >"$PAR2RS_OUT/par1-create-reject/source.txt"
  run_capture "$PAR2RS_OUT/par1-create-reject" "$WORK_DIR/par2rs-par1-create-reject" "$PAR2RS_BIN_DIR/par2" create out.par source.txt
  assert_nonzero_status "$WORK_DIR/par2rs-par1-create-reject"
  assert_absent "$PAR2RS_OUT/par1-create-reject/out.par"
}

if [[ "$HAS_TURBO" = 1 ]]; then
  run_case "verify intact PAR2" case_verify_intact_par2
  run_case "repair corrupted PAR2 file" case_repair_corrupted_par2_file
  run_case "report unrepairable missing PAR2 file" case_report_unrepairable_missing_par2_file
  run_case "reject create overwrite" case_reject_create_overwrite
  run_case "verify intact PAR1" case_verify_intact_par1
  run_case "verify PAR1 from volume input" case_verify_par1_from_volume_input
  run_case "repair missing PAR1 file" case_repair_missing_par1_file
  run_case "repair renamed PAR1 file" case_repair_renamed_par1_file
  run_case "purge intact PAR1" case_purge_intact_par1
  run_case "reject PAR1 create" case_reject_par1_create_self
else
  printf 'skipping turbo comparisons: %s is not executable or on PATH\n' "$TURBO_PAR2_CMD"
  run_case "verify intact PAR1" case_verify_intact_par1_self
  run_case "verify PAR1 from volume input" case_verify_par1_from_volume_input_self
  run_case "repair missing PAR1 file" case_repair_missing_par1_file_self
  run_case "repair renamed PAR1 file" case_repair_renamed_par1_file_self
  run_case "purge intact PAR1" case_purge_intact_par1_self
  run_case "reject PAR1 create" case_reject_par1_create_self
fi

printf 'comparison work dir: %s\n' "$WORK_DIR"

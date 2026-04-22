#!/usr/bin/env bash
set -euo pipefail

mode="${1:-html}"
shift || true

coverage_dir="${COVERAGE_DIR:-target/coverage}"
common_args=(--workspace --all-features)

usage() {
  cat <<'EOF'
Usage: ./scripts/coverage.sh [mode] [cargo-test-args...]

Modes:
  quick       Run tests with llvm-cov and print the terminal summary.
  html        Generate target/coverage/html/index.html.
  open        Generate the HTML report and open it in a browser.
  lcov        Generate target/coverage/lcov.info.
  cobertura   Generate target/coverage/cobertura.xml.
  codecov     Generate target/coverage/codecov.json.
  ci          Generate text, LCOV, and Cobertura reports under target/coverage.
  all|llvm    Generate text, HTML, LCOV, Cobertura, and Codecov reports.
  tests       Generate an HTML report for integration test targets only.
  tarpaulin   Generate Tarpaulin HTML and XML reports.
  both        Generate llvm-cov reports and Tarpaulin reports.
  clean       Remove coverage outputs and llvm-cov profiling artifacts.
EOF
}

require_llvm_cov() {
  if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "cargo-llvm-cov is required. Install it with: cargo install cargo-llvm-cov" >&2
    exit 1
  fi
}

require_tarpaulin() {
  if ! cargo tarpaulin --version >/dev/null 2>&1; then
    echo "cargo-tarpaulin is required. Install it with: cargo install cargo-tarpaulin" >&2
    exit 1
  fi
}

prepare_dir() {
  mkdir -p "${coverage_dir}"
}

run_llvm_tests_once() {
  require_llvm_cov
  prepare_dir
  cargo llvm-cov clean --workspace
  cargo llvm-cov "${common_args[@]}" --no-report --no-fail-fast "$@"
}

write_llvm_reports() {
  require_llvm_cov
  prepare_dir
  cargo llvm-cov report --text --output-path "${coverage_dir}/summary.txt"
  cargo llvm-cov report --lcov --output-path "${coverage_dir}/lcov.info"
  cargo llvm-cov report --cobertura --output-path "${coverage_dir}/cobertura.xml"
  cargo llvm-cov report --codecov --output-path "${coverage_dir}/codecov.json"
  cargo llvm-cov report --html --output-dir "${coverage_dir}"
  echo "Coverage reports written to ${coverage_dir}"
}

case "${mode}" in
  help|-h|--help)
    usage
    ;;
  quick)
    require_llvm_cov
    cargo llvm-cov "${common_args[@]}" "$@"
    ;;
  html)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --html --output-dir "${coverage_dir}" "$@"
    echo "HTML coverage report: ${coverage_dir}/html/index.html"
    ;;
  open)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --open --output-dir "${coverage_dir}" "$@"
    ;;
  lcov)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --lcov --output-path "${coverage_dir}/lcov.info" "$@"
    echo "LCOV coverage report: ${coverage_dir}/lcov.info"
    ;;
  cobertura)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --cobertura --output-path "${coverage_dir}/cobertura.xml" "$@"
    echo "Cobertura coverage report: ${coverage_dir}/cobertura.xml"
    ;;
  codecov)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --codecov --output-path "${coverage_dir}/codecov.json" "$@"
    echo "Codecov JSON coverage report: ${coverage_dir}/codecov.json"
    ;;
  ci)
    run_llvm_tests_once "$@"
    cargo llvm-cov report --text --output-path "${coverage_dir}/summary.txt"
    cargo llvm-cov report --lcov --output-path "${coverage_dir}/lcov.info"
    cargo llvm-cov report --cobertura --output-path "${coverage_dir}/cobertura.xml"
    cargo llvm-cov report
    echo "CI coverage reports written to ${coverage_dir}"
    ;;
  all|llvm)
    run_llvm_tests_once "$@"
    write_llvm_reports
    ;;
  tests)
    require_llvm_cov
    prepare_dir
    cargo llvm-cov "${common_args[@]}" --tests --html --output-dir "${coverage_dir}/tests" "$@"
    echo "Integration test coverage report: ${coverage_dir}/tests/html/index.html"
    ;;
  tarpaulin)
    require_tarpaulin
    prepare_dir
    cargo tarpaulin \
      --workspace \
      --all-features \
      --out Html \
      --out Xml \
      --output-dir "${coverage_dir}/tarpaulin" \
      --skip-clean \
      "$@"
    echo "Tarpaulin reports written to ${coverage_dir}/tarpaulin"
    ;;
  both)
    run_llvm_tests_once "$@"
    write_llvm_reports
    require_tarpaulin
    cargo tarpaulin \
      --workspace \
      --all-features \
      --out Html \
      --out Xml \
      --output-dir "${coverage_dir}/tarpaulin" \
      --skip-clean \
      "$@"
    echo "Tarpaulin reports written to ${coverage_dir}/tarpaulin"
    ;;
  clean)
    require_llvm_cov
    rm -rf "${coverage_dir}"
    cargo llvm-cov clean --workspace
    echo "Removed ${coverage_dir} and llvm-cov profiling artifacts"
    ;;
  *)
    echo "Unknown coverage mode: ${mode}" >&2
    usage >&2
    exit 2
    ;;
esac

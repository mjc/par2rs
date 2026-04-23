# par2cmdline-turbo Parity

This document tracks pragmatic compatibility with the local
`/home/mjc/projects/par2cmdline-turbo` checkout. The goal is compatible command
behavior and filesystem effects, not byte-for-byte stdout or PAR packet layout.

## Supported Binaries

| Binary | Status | Notes |
| --- | --- | --- |
| `par2` | Supported | Subcommands `create`/`c`, `verify`/`v`, and `repair`/`r`. |
| `par2create` | Supported | Standalone create-compatible entry point. |
| `par2verify` | Supported | PAR2 verify plus PAR1 verify dispatch. |
| `par2repair` | Supported | PAR2 repair plus PAR1 repair dispatch. |

## Shared CLI Semantics

- `-v` selects noisy logging and `-vv` selects debug logging.
- `-q` selects quiet output and `-qq` selects silent output.
- Mixing `-v` and `-q` is rejected by all supported entry points.
- `RUST_LOG` takes precedence over CLI-derived logging.
- `-T`/`--file-threads` must be positive.
- `-m`/`--memory` is parsed as MiB and must be positive when present.
- `--threads 0` remains accepted for existing auto-thread semantics.

## Create

Supported create forms:

- `par2 create [options] <base.par2> <files...>`
- `par2 c [options] <base.par2> <files...>`
- `par2create [options] <base.par2> <files...>`

Important semantics:

- Existing output files are rejected before creation starts.
- The library defaults to overwrite refusal through
  `CreateConfig::overwrite_existing = false`.
- The library builder can opt into replacement with
  `CreateContextBuilder::overwrite_existing(true)`.
- The CLI intentionally exposes no overwrite option.
- Output files are opened with create-new semantics to avoid race-condition
  truncation.
- `-T` bounds file-level hashing work. `-t` remains CPU/Reed-Solomon thread
  configuration.

Supported create options include `-a`, `-b`, `-s`, `-r`, `-c`, `-f`, `-u`,
`-l`, `-n`, `-R`, `-B`, `-m`, `-t`, `-T`, `-v`, `-q`, and `--force-scalar`.
`-O` is rejected for create commands.

Known create differences:

- PAR1 creation is not implemented. PAR1-named create outputs are rejected with
  `PAR1 create is not supported`.
- Exact volume packet ordering and stdout wording are not compatibility
  targets.

## Verify And Repair

Supported verify forms:

- `par2 verify [options] <set.par2|set.par|set.pNN> [extra files...]`
- `par2 v [options] <set.par2|set.par|set.pNN> [extra files...]`
- `par2verify [options] <set.par2|set.par|set.pNN> [extra files...]`

Supported repair forms:

- `par2 repair [options] <set.par2|set.par|set.pNN> [extra files...]`
- `par2 r [options] <set.par2|set.par|set.pNN> [extra files...]`
- `par2repair [options] <set.par2|set.par|set.pNN> [extra files...]`

Important semantics:

- `-S` is valid only with `-N` for PAR2 verify/repair.
- `-N` with no `-S` defaults skip leeway to `64`.
- `-O` rename-only mode is supported for PAR2 verify and repair.
- Without `-N`, byte scanning remains exhaustive.
- With `-N`, scan skip-ahead follows the turbo-style
  `min(skip_leeway * 2, block_size)` scan distance.
- Final PAR2 file status validates full MD5, first-16K MD5, and size. A file
  with all block hashes found but a mismatched whole-file hash is corrupted.
- Extra files are marked renamed only on exact size/full-MD5/16K-MD5 matches.
- PAR2 verify is non-mutating. Exact renamed extra files count as available
  data in the report, but verify exits with repair-required status until repair
  moves them into the protected target path.
- PAR2 repair consumes exact renamed extra files by moving them into the
  expected protected path before Reed-Solomon reconstruction is attempted.
- If a corrupted protected target already exists, PAR2 repair first moves it to
  the first free `filename.N` backup path, then moves the exact renamed extra
  into place.
- PAR2 `-O` repair performs rename moves only. It does not reconstruct from
  recovery blocks and fails if any protected file remains missing or corrupted.
- PAR2 verify with `-O` is non-mutating and considers extra files only as
  perfect renamed matches.
- Repair memory limits cap reconstruction chunk size only when explicitly set.
- `-T` bounds verify file scanning with a local Rayon pool where applicable.
- PAR1 verify and repair accept `-O` for command-line compatibility but ignore
  it internally.
- PAR1 verify and repair scan command-line extra files for exact wrong-name
  matches. Repair renames those files into place before using recovery blocks.
- PAR2 purge after repair-by-rename deletes collected PAR2 recovery files and
  backups created by that repair operation when repair succeeds. Failed repair
  does not purge. Data files are never deleted except wrong-name extras moved
  into the correct protected target path.
- PAR1 purge runs only after a successful verify or repair. It deletes only the
  PAR1 files collected for the input set (`.par`/`.pNN`) and backups created by
  the same repair-by-rename operation. Data files are never purge targets, and
  failed verify or repair operations do not purge.

Known verify/repair differences:

- PAR1 ignores file-thread and skip-leeway options.
- PAR1 output is functional rather than byte-for-byte matched to turbo.
- Exact numeric failure codes for invalid syntax or failed repair are not a
  compatibility target; parity checks require matching success/failure outcome
  and filesystem effects.
- Byte-for-byte stdout/help parity remains outside the target.

## PAR1 Status

Implemented:

- Format detection for `.par`, `.PAR`, `.pNN`, and `.PNN` inputs.
- Related PAR1 file collection from either main or volume input.
- Native Rust parser for PAR1 headers, file entries, control hash, set hash,
  volume metadata, recovery payloads, and UTF-16LE names.
- Verify by size, full MD5, and first-16K MD5.
- Extra-file renamed detection for exact size/full-MD5/16K-MD5 matches, with
  PAR1 recovery files ignored as extra candidates.
- Repair by renaming exact wrong-name files into the protected target path,
  including deterministic numbered backups for corrupted targets.
- Whole-file repair for missing or corrupted protected files using existing
  Reed-Solomon primitives and PAR1 recovery volumes.
- Purge after successful verify or repair, limited to collected PAR1 recovery
  files and backups created during that repair run.
- Tests using real PAR1 fixtures from the local turbo checkout.

Not implemented:

- PAR1 create.

## Performance Backend Status

Bench coverage exists for:

- Create hashing and PAR2 generation: `benches/create_benchmark.rs`.
- Verify hash paths: `benches/verify_performance.rs`,
  `benches/md5_throughput.rs`, and `benches/md5_optimized.rs`.
- Repair reconstruction, matrix inversion, GF16 arithmetic, and SIMD
  multiply-add: `benches/repair_benchmark.rs`.

Backend parity:

| Turbo/ParPar feature | Status |
| --- | --- |
| Stitched MD5+CRC32 | Approximated; create/verify share checksum helpers but do not claim full ParPar stitching parity. |
| GF16 internal RAM-error checksumming | Not implemented. |
| AVX2/SSSE3-style multiply-add acceleration | Implemented on supported x86_64 paths, with scalar fallback. |
| Portable SIMD fallback | Implemented where available. |
| AVX512/RISC-V breadth | Not implemented. |
| Matrix inversion acceleration | Benchmarked; no new algorithmic replacement in this slice. |

Current benchmark result history is in `docs/BENCHMARK_RESULTS.md`. Fresh local
results should be captured with `cargo bench` and recorded there before making
performance claims.

## Local Parity Check

Use:

```sh
scripts/compare_turbo_parity.sh
```

The script builds release binaries, runs selected cases against local turbo and
par2rs, compares exit codes and key filesystem effects, and avoids exact stdout
matching. By default it re-execs itself through `nix develop` so turbo and cargo
come from the project environment rather than the host `PATH`. Set
`PARITY_COMPARE_USE_NIX=0` to disable that wrapper, and set
`TURBO_PAR2=/path/to/par2` or `TURBO_ROOT=/path/to/turbo` when overriding the
turbo binary.

Current script coverage includes:

- Top-level `par2 -V`/`-VV` acceptance.
- Valid `-q`, `-qq`, `-v`, and `-vv` noise options across create,
  verify/repair, and standalone wrappers, plus mixed verbose/quiet rejection.
- PAR2 create through `create`, `c`, and `par2create`, including `-a`, `-B`,
  `-R`, `--` hyphen-prefixed input, `-b`, `-s`, `-r` percent and size targets,
  `-c`, `-f`, `-u`, `-l`, `-n`, `-T`, `-t`, and `-m`.
- Exact generated recovery filename layouts for `-u`, `-l`, `-n`, `-f`, and
  `-c0`, with representative standalone `par2create` layout checks.
- Standalone `par2create` filesystem effects for the valid create option
  matrix, including recursive input and index-only `-c0` creation.
- Invalid PAR2 create combinations and ranges, including block count/size
  conflicts, duplicate singleton options, invalid redundancy suffixes,
  recovery-file layout conflicts, verify/repair-only option rejection, and
  output index/volume overwrite refusal. Standalone `par2create` covers the
  same invalid matrix and overwrite refusal cases.
- PAR2 verify/repair through full commands, `v`/`r` aliases, `par2verify`, and
  `par2repair`, from PAR2 set input and protected data-file input, including
  PAR2 volume input, uppercase `.PAR2` main and volume input, `-B`, `-N`,
  `-N -S`, `-T`, `-m`, `--` hyphen-prefixed extra files, renamed-file repair
  from main and volume input, standalone rename-only repair, rename-only
  verify/repair, damaged rename-only failure, unrepairable missing-file
  reporting, and purge after intact verify or successful repair.
- PAR2 purge effects after repair-by-rename, repair-by-rename with a corrupted
  target backup, and failed repair where recovery files must remain.
- Invalid PAR2 verify/repair options, including `-S` without `-N`, invalid
  `-S`, standalone `-S` rejection, create-only option rejection, invalid `-T`,
  and invalid `-m`.
- PAR1 verify from main and volume input, uppercase `.PAR`/`.PNN` input,
  missing-file repair, repair from volume input, renamed-file repair from main
  and volume input, purge after verify and repair, failed repair with purge
  preserving recovery files, and `-O` acceptance. Standalone `par2verify` and
  `par2repair` PAR1 dispatch is covered for verify, repair, renamed-file
  repair, and `-O` acceptance.
- par2rs self-check for intentional PAR1 create rejection. The Nix turbo binary
  treats `out.par` as a PAR2 basename and writes `out.par.par2`, so this case is
  intentionally not a turbo status comparison.

## Current Acceptance Commands

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --test test_binaries
cargo test --test test_create_integration
cargo test --test test_global_verification_scanning_bugs
cargo test --test test_repair_integration
cargo bench --no-run
scripts/compare_turbo_parity.sh
```

Run the same commands through `nix develop -c` when validating in the Nix
environment requested by the final parity plan.

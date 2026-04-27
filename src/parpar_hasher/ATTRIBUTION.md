# Attribution: par2rs `parpar_hasher` module

The Rust source files in this directory are line-for-line ports of C++
headers from the **par2cmdline-turbo** / **ParPar** projects, both
distributed under **GNU GPL v2 or later**. par2rs is also distributed
under **GNU GPL v2 or later**, so the licenses are compatible.

## Upstream

* Project: par2cmdline-turbo
  * Repo: <https://github.com/animetosho/par2cmdline-turbo>
  * License: GPL-2.0-or-later (`COPYING` at the repo root)
* Project: ParPar (the upstream of par2cmdline-turbo's `parpar/` subtree)
  * Repo: <https://github.com/animetosho/ParPar>
  * License: GPL-2.0-or-later

## File-by-file mapping

| par2rs Rust source            | Upstream C/C++ source                     | Notes                                                |
| ----------------------------- | ----------------------------------------- | ---------------------------------------------------- |
| `md5x2_scalar.rs`             | `parpar/hasher/md5x2-x86-asm.h`           | Two-lane scalar (GPR) MD5 via `asm!`.                |
| *not ported* — see note below | `parpar/hasher/crc_clmul.h`               | Superseded by `crc32fast` crate (already faster).    |
| `hasher_input.rs`             | `parpar/hasher/hasher_input_base.h`,      | Fused 64-byte driver (block-MD5 + file-MD5 + CRC32). |
|                               | `parpar/hasher/hasher_input.cpp`,         |                                                      |
|                               | `parpar/hasher/hasher_input_impl.h`       |                                                      |
| `md5x2_sse.rs` *(future)*     | `parpar/hasher/md5x2-sse-asm.h`           | SSE2 two-lane MD5 (each lane in `__m128i`).          |
| `md5x2_avx512.rs` *(future)*  | `parpar/hasher/md5-avx512-asm.h`          | AVX-512 ternary-logic-accelerated path.              |
| `md5x2_neon.rs` *(future)*    | `parpar/hasher/md5x2-neon-asm.h`,         | aarch64 NEON two-lane MD5.                           |
|                               | `parpar/hasher/md5-arm64-asm.h`           |                                                      |
| `crc_arm.rs` *(future)*       | `parpar/hasher/crc_arm.h`                 | aarch64 PMULL CRC32 fold (deferred — `crc32fast` covers ARM via the same fallback).            |

## CRC32 backend decision (T2.b)

Rather than porting `parpar/hasher/crc_clmul.h` (PCLMULQDQ fold) and
`parpar/hasher/crc_arm.h` (PMULL fold), par2rs uses the existing
[`crc32fast`](https://crates.io/crates/crc32fast) crate. A microbench
(`benches/crc_compare.rs`) compared `crc32fast` against the alternative
[`crc-fast`](https://crates.io/crates/crc-fast) crate (which folds 8-at-a-time
vs. parpar's 4-at-a-time) on the access patterns par2rs actually uses:

* 64 B one-shot — `crc32fast` ~50% faster.
* 16 KiB streamed in 64 B chunks — `crc32fast` ~17% faster.
* 4 MiB streamed in 64 B chunks — `crc32fast` ~13% faster.
* 4–64 MiB single-shot — `crc-fast` ~6–8% faster (irrelevant: not our
  access pattern, since the fused HasherInput driver feeds 64 B at a time
  to interleave with MD5x2).

Conclusion: the fused 64-B inner-loop path is `crc32fast`'s strong
regime, so no port and no swap. `crc-fast` is retained as a `dev-dep`
purely so `benches/crc_compare.rs` can be re-run by future contributors
who want to revisit the decision.

A second bench, `benches/md5x2_crc_fused.rs`, measures the choice
inside the *actual* `HasherInput` access pattern (MD5x2 + CRC32
interleaved at 64 B granularity over `(file-MD5, block-MD5, block-CRC32)`).
There the two CRC backends are within noise of each other (816 vs 808
MiB/s at 16 KiB; 819 vs 823 MiB/s at 4 MiB) — MD5x2's GPR work hides
the CRC backend cost difference entirely. Decision unchanged: use
`crc32fast`.

That same bench surfaced an unrelated finding worth noting here: the
currently shipped Tier-1 helper
`checksum::update_file_md5_block_md5_crc32_fused` runs at ~494 MiB/s
vs ~954 MiB/s for a naive 3-pass sequential at 16 KiB / 4 MiB. Cache
traffic improved (per `perf stat`), wall-clock did not. Tracked as a
follow-up; T2.c will likely replace this helper on the create path.

## What was copied vs. re-derived

* The instruction sequences inside each `asm!` block are direct
  translations of the corresponding upstream `asm volatile` blocks: same
  ordering, same register roles, same constant tables.
* The Rust glue around them (struct layout, function signatures, error
  handling, `#[cfg]` gating) is fresh par2rs code.
* MD5 round constants and the 64 K table are the standard values from
  RFC 1321 and were not copied as such.

Each Rust file carries a header comment naming its specific upstream
source file and giving the upstream project's copyright line so the
provenance is greppable from inside the repo.

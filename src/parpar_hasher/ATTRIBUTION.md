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
| `crc_clmul.rs`                | `parpar/hasher/crc_clmul.h`               | PCLMULQDQ CRC32 fold. Originally Intel zlib-style.   |
| `hasher_input.rs`             | `parpar/hasher/hasher_input_base.h`,      | Fused 64-byte driver (block-MD5 + file-MD5 + CRC32). |
|                               | `parpar/hasher/hasher_input.cpp`,         |                                                      |
|                               | `parpar/hasher/hasher_input_impl.h`       |                                                      |
| `md5x2_sse.rs` *(future)*     | `parpar/hasher/md5x2-sse-asm.h`           | SSE2 two-lane MD5 (each lane in `__m128i`).          |
| `md5x2_avx512.rs` *(future)*  | `parpar/hasher/md5-avx512-asm.h`          | AVX-512 ternary-logic-accelerated path.              |
| `md5x2_neon.rs` *(future)*    | `parpar/hasher/md5x2-neon-asm.h`,         | aarch64 NEON two-lane MD5.                           |
|                               | `parpar/hasher/md5-arm64-asm.h`           |                                                      |
| `crc_arm.rs` *(future)*       | `parpar/hasher/crc_arm.h`                 | aarch64 PMULL CRC32 fold.                            |

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

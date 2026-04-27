//! ParPar-style fused MD5x2 + CRC32 hasher for the PAR2 create path.
//!
//! This module is a Rust port of the hashing core from
//! [par2cmdline-turbo](https://github.com/animetosho/par2cmdline-turbo) /
//! [ParPar](https://github.com/animetosho/ParPar) (both GPL-2.0-or-later).
//! The original C++ source lives under `parpar/hasher/` in those projects;
//! per-module attribution headers point at the specific upstream files.
//!
//! ## Why this exists
//!
//! The PAR2 create path needs three hashes computed over every source byte:
//!
//! * a **per-block MD5** (one MD5 per source block, reset between blocks),
//! * a **per-block CRC32** (one CRC32 per source block),
//! * a **per-file MD5** (one MD5 per source file, spans every block).
//!
//! Doing them as three separate passes evicts the source bytes from L1/L2
//! between passes. ParPar's trick is:
//!
//! 1. Use a two-lane MD5 (MD5x2) that runs both MD5s in parallel for free
//!    via instruction-level parallelism in a single core, so the per-block
//!    and per-file MD5 share one walk over the data.
//! 2. Fuse that with a CLMul CRC32 update at the same 64-byte granularity
//!    so all three states advance from one cache-line read.
//!
//! This module exposes the resulting `HasherInput` API that par2rs's
//! `create::context` feeds source bytes into.
//!
//! ## Layout
//!
//! * `md5x2_scalar` — two-lane MD5 using GPR-only `asm!` (works on any
//!   x86_64 CPU). Mirrors `parpar/hasher/md5x2-x86-asm.h`.
//! * `crc_clmul` — x86_64 PCLMULQDQ CRC32 (4-fold). Mirrors
//!   `parpar/hasher/crc_clmul.h`. Used by the fused driver at 64 B
//!   granularity to avoid the per-call SIMD-setup overhead a generic
//!   crate (`crc32fast`) would pay 65 536× per 4 MiB block. See
//!   `ATTRIBUTION.md` (T2.b → T2.b' decision) and
//!   `benches/md5x2_crc_fused.rs` for the data behind that choice.
//!   `crc32fast` is still used for bulk CRC outside the fused driver.
//! * `hasher_input` — the fused 64-byte driver that owns one MD5x2 state
//!   and one CRC32 state per source file, plus the staggered-offset
//!   bookkeeping (`tmp` / `posOffset` / `tmpLen`) that lets the two MD5
//!   lanes advance independently. Mirrors
//!   `parpar/hasher/hasher_input_base.h` + `hasher_input.cpp`.
//!
//! Future tiers (SSE2 MD5x2, AVX-512 MD5x2, aarch64 NEON MD5x2) layer on
//! top via runtime dispatch keyed off `is_x86_feature_detected!` /
//! `std::arch::is_aarch64_feature_detected!`. The scalar path is the
//! always-available baseline.

#![allow(dead_code)] // scaffold; populated incrementally

#[cfg(target_arch = "x86_64")]
pub mod crc_clmul;
#[cfg(target_arch = "x86_64")]
pub mod hasher_input;
#[cfg(target_arch = "x86_64")]
pub mod md5x2;
#[cfg(target_arch = "x86_64")]
pub mod md5x2_scalar;
#[cfg(target_arch = "x86_64")]
pub mod md5x2_sse2;

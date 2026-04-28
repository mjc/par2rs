//! Runtime-CPU-dispatched `HasherInput`.
//!
//! Mirrors par2cmdline-turbo's `HasherInput_Create()` factory at
//! `parpar/hasher/hasher.cpp:96` — a tiny dispatch layer that picks the
//! best concrete `HasherInput<Backend>` for the CPU at construction
//! time and then delegates every method to it.
//!
//! Upstream returns an `IHasherInput*` (vtable). We use an enum so the
//! compiler can see the concrete type at every call site — one extra
//! match in the prologue, but the per-byte work then runs through the
//! statically-known backend with no vtable indirection.
//!
//! Selection rules match upstream's big-core branch (see
//! `hasher.cpp:105-117`):
//!
//! * `avx512f+vl+pclmulqdq+sse4.1` available AND CPU is not a known
//!   slow-`vprold` part (Zen4 family `0x19` / Zen5 family `0x1A`)
//!   → `Avx512` backend (preferred on Intel Skylake-X+, Ice Lake+,
//!   Sapphire Rapids+, and any future AMD with fast vector rotate).
//! * else `bmi1` available → `Bmi1` backend.
//! * else → `Scalar` backend (always-available baseline).
//!
//! `Sse2` is not selected automatically: on Zen3 it is slower than
//! `Scalar` (see `benches/parpar_hasher_input.rs`), and upstream only
//! picks SSE on small cores (Tremont / Gracemont / Zen1) which the
//! current target audience (5950X) doesn't include. The variant is
//! kept reachable for future small-core dispatch and as a correctness
//! oracle for the scalar/BMI1 paths.

use super::hasher_input::{BlockHash, HasherInput};
use super::md5x2_avx512::Avx512;
use super::md5x2_bmi1::Bmi1;
use super::md5x2_scalar::Scalar;
use super::md5x2_sse2::Sse2;

/// Runtime-dispatched fused MD5x2 + CRC32 hasher.
///
/// Construct via [`HasherInputDyn::new`], then drive with `update` /
/// `get_block` / `end` — same surface as [`HasherInput`].
#[allow(clippy::large_enum_variant)] // backends are similar-sized; enum picked at construction
pub enum HasherInputDyn {
    Scalar(HasherInput<Scalar>),
    Sse2(HasherInput<Sse2>),
    Bmi1(HasherInput<Bmi1>),
    Avx512(HasherInput<Avx512>),
}

impl HasherInputDyn {
    /// Pick the best backend for the current CPU and construct it.
    /// Mirrors upstream `HasherInput_Create()` for the big-core branch.
    ///
    /// Selection (preferred → fallback):
    /// 1. **AVX-512VL** — when avx512f+vl+pclmulqdq+sse4.1 are present
    ///    AND the CPU is NOT an AMD Zen4/Zen5 part. Upstream's
    ///    `setup_hasher` excludes Zen4/Zen5 from this tier via
    ///    `isVecRotSlow` (`vprold` has 2-cycle latency there, slower
    ///    than the BMI1 path). Since AMD CPUs with AVX-512 are
    ///    *exclusively* Zen4/Zen5 (Zen3 and earlier have no AVX-512),
    ///    we exclude AMD entirely from this tier.
    /// 2. **BMI1** — fast non-AVX-512 fallback.
    /// 3. **Scalar** — last resort.
    pub fn new() -> Self {
        if Self::should_use_avx512() {
            HasherInputDyn::Avx512(HasherInput::new())
        } else if is_x86_feature_detected!("bmi1") {
            HasherInputDyn::Bmi1(HasherInput::new())
        } else {
            HasherInputDyn::Scalar(HasherInput::new())
        }
    }

    /// Returns true iff the CPU has the full feature set required by
    /// the AVX-512VL HasherInput backend.
    fn avx512_supported() -> bool {
        is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1")
    }

    /// `true` iff we should auto-select the AVX-512VL backend on this
    /// CPU. Combines feature detection with the `isVecRotSlow`
    /// exclusion (Zen4/Zen5 ⇒ slow `vprold`).
    fn should_use_avx512() -> bool {
        Self::avx512_supported() && !is_amd_vec_rot_slow()
    }

    /// Force the `Scalar` backend regardless of CPU support. Useful in
    /// tests and as a correctness oracle.
    pub fn new_scalar() -> Self {
        HasherInputDyn::Scalar(HasherInput::new())
    }

    /// Force the `Sse2` backend (requires SSE2; always available on
    /// x86_64). Mainly useful for cross-checks.
    pub fn new_sse2() -> Self {
        HasherInputDyn::Sse2(HasherInput::new())
    }

    /// Force the `Bmi1` backend. Requires BMI1; will not be selected
    /// by `new()` on non-BMI1 hosts. Caller is responsible for the
    /// CPU check.
    pub fn new_bmi1() -> Self {
        HasherInputDyn::Bmi1(HasherInput::new())
    }

    /// Force the `Avx512` backend. Requires avx512f + avx512vl +
    /// pclmulqdq + sse4.1.
    ///
    /// # Safety
    /// Calling any method on the returned value (`update`, `get_block`,
    /// `end`) on a CPU lacking the required features will execute
    /// unsupported instructions (UB / SIGILL). Prefer
    /// [`HasherInputDyn::try_new_avx512`] which checks at runtime.
    pub unsafe fn new_avx512() -> Self {
        HasherInputDyn::Avx512(HasherInput::new())
    }

    /// Safe AVX-512 constructor: returns `Some` iff the CPU supports
    /// the full AVX-512VL HasherInput feature set. This bypasses the
    /// AMD/Zen4 exclusion in `new()` — use only when you specifically
    /// want to exercise the AVX-512 path (e.g. tests, benches,
    /// micro-benchmarks evaluating whether the exclusion still holds).
    pub fn try_new_avx512() -> Option<Self> {
        if Self::avx512_supported() {
            // SAFETY: we just checked the feature set.
            Some(unsafe { Self::new_avx512() })
        } else {
            None
        }
    }

    /// Identifies which backend was selected. Mostly for diagnostics.
    pub fn backend_name(&self) -> &'static str {
        match self {
            HasherInputDyn::Scalar(_) => "scalar",
            HasherInputDyn::Sse2(_) => "sse2",
            HasherInputDyn::Bmi1(_) => "bmi1",
            HasherInputDyn::Avx512(_) => "avx512",
        }
    }

    /// Feed bytes. See [`HasherInput::update`].
    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        match self {
            HasherInputDyn::Scalar(h) => h.update(data),
            HasherInputDyn::Sse2(h) => h.update(data),
            HasherInputDyn::Bmi1(h) => h.update(data),
            HasherInputDyn::Avx512(h) => h.update(data),
        }
    }

    /// Finalize the current block and reset the per-block state.
    /// See [`HasherInput::get_block`].
    #[inline]
    pub fn get_block(&mut self, zero_pad: u64) -> BlockHash {
        match self {
            HasherInputDyn::Scalar(h) => h.get_block(zero_pad),
            HasherInputDyn::Sse2(h) => h.get_block(zero_pad),
            HasherInputDyn::Bmi1(h) => h.get_block(zero_pad),
            HasherInputDyn::Avx512(h) => h.get_block(zero_pad),
        }
    }

    /// Finalize the file-MD5 and consume self.
    /// See [`HasherInput::end`].
    #[inline]
    pub fn end(self) -> [u8; 16] {
        match self {
            HasherInputDyn::Scalar(h) => h.end(),
            HasherInputDyn::Sse2(h) => h.end(),
            HasherInputDyn::Bmi1(h) => h.end(),
            HasherInputDyn::Avx512(h) => h.end(),
        }
    }
}

impl Default for HasherInputDyn {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if this CPU is a known AMD part with slow vector
/// rotate (`vprold` ≥ 2 cycles latency), which makes the AVX-512VL
/// HasherInput backend slower than BMI1. Mirrors upstream
/// `setup_hasher`'s `isVecRotSlow` check (`hasher.cpp` + `cpu.h`).
///
/// **Currently slow:** Zen4 (family `0x19`) and Zen5 (family `0x1A`).
/// Zen3 and earlier have no AVX-512 at all, so they cannot reach this
/// code path. Future AMD families (Zen6+, family `0x1B` and above) are
/// **not** excluded — we assume they fix the rotate latency until
/// proven otherwise.
fn is_amd_vec_rot_slow() -> bool {
    // SAFETY: CPUID is a baseline x86 instruction; safe on every
    // x86_64 CPU that can run this binary.
    unsafe {
        use core::arch::x86_64::__cpuid;
        // Leaf 0: vendor string in EBX/EDX/ECX.
        let v = __cpuid(0);
        let vendor = [
            v.ebx.to_le_bytes(),
            v.edx.to_le_bytes(),
            v.ecx.to_le_bytes(),
        ];
        let vendor_flat: [u8; 12] = [
            vendor[0][0],
            vendor[0][1],
            vendor[0][2],
            vendor[0][3],
            vendor[1][0],
            vendor[1][1],
            vendor[1][2],
            vendor[1][3],
            vendor[2][0],
            vendor[2][1],
            vendor[2][2],
            vendor[2][3],
        ];
        if &vendor_flat != b"AuthenticAMD" {
            return false;
        }
        // Leaf 1 EAX: extended_family[27:20], extended_model[19:16],
        // family[11:8], model[7:4]. AMD effective family = base +
        // extended (the conditional add only applies when base==0xF
        // for Intel, but AMD always sums them per AMD's CPUID spec
        // §1.3.2 — though for base<0xF the extended bits are 0
        // anyway, so unconditional sum is safe).
        let eax = __cpuid(1).eax;
        let base_family = ((eax >> 8) & 0xF) as u32;
        let ext_family = ((eax >> 20) & 0xFF) as u32;
        let family = base_family + ext_family;
        // Zen4 = 0x19, Zen5 = 0x1A. Both have slow vprold.
        matches!(family, 0x19 | 0x1A)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_a_backend() {
        let h = HasherInputDyn::new();
        // Just ensure construction works.
        let name = h.backend_name();
        assert!(matches!(name, "scalar" | "sse2" | "bmi1" | "avx512"));
    }

    #[test]
    fn dyn_matches_scalar_for_simple_stream() {
        // Feed identical bytes through `new()` and `new_scalar()`,
        // assert per-block hashes and final file MD5 match. This
        // exercises whichever backend `new()` selected against the
        // canonical scalar path.
        let data: Vec<u8> = (0..4096u32).map(|i| (i * 31 + 7) as u8).collect();
        let block_size = 1024usize;

        let mut a = HasherInputDyn::new();
        let mut b = HasherInputDyn::new_scalar();

        let mut a_blocks = Vec::new();
        let mut b_blocks = Vec::new();
        for chunk in data.chunks(block_size) {
            a.update(chunk);
            b.update(chunk);
            a_blocks.push(a.get_block(0));
            b_blocks.push(b.get_block(0));
        }
        assert_eq!(a_blocks, b_blocks);

        let a_file = a.end();
        let b_file = b.end();
        assert_eq!(a_file, b_file);
    }

    /// Explicit cross-check: force the AVX-512 backend and compare
    /// against scalar over a multi-block stream including a partial
    /// tail. `dyn_matches_scalar_*` only tests whichever backend
    /// `new()` picked, so on non-AVX-512 hosts the AVX-512 path
    /// otherwise stays untouched.
    ///
    /// `#[ignore]` so non-AVX-512 hosts don't silently report a green
    /// pass for a path that wasn't exercised. Run explicitly with
    /// `cargo test -- --ignored dyn_avx512_matches_scalar` on
    /// AVX-512-capable hardware.
    #[test]
    #[ignore = "requires AVX-512VL hardware; run with --ignored"]
    fn dyn_avx512_matches_scalar() {
        let avx512 = match HasherInputDyn::try_new_avx512() {
            Some(h) => h,
            None => panic!(
                "dyn_avx512_matches_scalar invoked on CPU lacking \
                 avx512f+vl+pclmulqdq+sse4.1 — re-run on AVX-512 hardware"
            ),
        };
        assert_eq!(avx512.backend_name(), "avx512");

        // 5 blocks of 1024 + a 333-byte tail — exercises the steady-state
        // 64 B fused loop, get_block at staggered offsets, and the
        // partial-block tail path.
        let total = 5 * 1024 + 333;
        let data: Vec<u8> = (0..total as u32).map(|i| (i * 37 + 11) as u8).collect();
        let block_size = 1024usize;

        let mut a = avx512;
        let mut b = HasherInputDyn::new_scalar();

        let mut a_blocks = Vec::new();
        let mut b_blocks = Vec::new();
        for chunk in data.chunks(block_size) {
            a.update(chunk);
            b.update(chunk);
            let pad = (block_size - chunk.len()) as u64;
            a_blocks.push(a.get_block(pad));
            b_blocks.push(b.get_block(pad));
        }
        assert_eq!(a_blocks, b_blocks, "avx512 vs scalar block hashes diverge");
        assert_eq!(a.end(), b.end(), "avx512 vs scalar file MD5 diverges");
    }

    #[test]
    fn zero_pad_behavior() {
        // Last block shorter than block_size — caller passes zero_pad
        // for the BLOCK hashes only, file MD5 must skip the pad.
        let block_size = 64usize;
        let mut data = vec![0u8; 100]; // one full block + 36 byte tail
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(13);
        }

        let mut h = HasherInputDyn::new();
        h.update(&data[..block_size]);
        let _b0 = h.get_block(0);
        h.update(&data[block_size..]);
        let _b1 = h.get_block((block_size - (data.len() - block_size)) as u64);
        let file_md5 = h.end();

        // Cross check file MD5 against md-5 crate fed the real bytes only.
        use md5::{Digest, Md5};
        let mut m = Md5::new();
        m.update(&data);
        let expected = m.finalize();
        assert_eq!(&file_md5[..], &expected[..]);
    }
}

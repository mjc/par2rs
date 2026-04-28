//! Runtime-CPU-dispatched `HasherInput`.
//!
//! Mirrors par2cmdline-turbo's `HasherInput_Create()` factory — picks the
//! best concrete `HasherInput<Backend>` for the CPU at construction time
//! and delegates every method to it.
//!
//! This dispatcher is architecture-agnostic; selection logic branches on
//! `target_arch`. x86_64 dispatches to Scalar/SSE2/BMI1/AVX-512; aarch64
//! dispatches to Scalar (with NEON and ARM CRC placeholders for future).

use super::hasher_input::{BlockHash, HasherInput};

// ============================================================================
// x86_64 Dispatcher
// ============================================================================

#[cfg(target_arch = "x86_64")]
pub enum HasherInputDyn {
    Scalar(HasherInput<super::md5x2_scalar::Scalar>),
    Sse2(HasherInput<super::md5x2_sse2::Sse2>),
    Bmi1(HasherInput<super::md5x2_bmi1::Bmi1>),
    Avx512(HasherInput<super::md5x2_avx512::Avx512>),
}

#[cfg(target_arch = "x86_64")]
impl HasherInputDyn {
    /// Pick the best backend for the current CPU and construct it.
    ///
    /// Selection (preferred → fallback):
    /// 1. **AVX-512VL** — when avx512f+vl+pclmulqdq+sse4.1 are present
    ///    AND the CPU is NOT an AMD Zen4/Zen5 part.
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

    fn avx512_supported() -> bool {
        is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512vl")
            && is_x86_feature_detected!("pclmulqdq")
            && is_x86_feature_detected!("sse4.1")
    }

    fn should_use_avx512() -> bool {
        Self::avx512_supported() && !is_amd_vec_rot_slow()
    }

    pub fn new_scalar() -> Self {
        HasherInputDyn::Scalar(HasherInput::new())
    }

    pub fn new_sse2() -> Self {
        HasherInputDyn::Sse2(HasherInput::new())
    }

    pub fn new_bmi1() -> Self {
        HasherInputDyn::Bmi1(HasherInput::new())
    }

    pub unsafe fn new_avx512() -> Self {
        HasherInputDyn::Avx512(HasherInput::new())
    }

    pub fn try_new_avx512() -> Option<Self> {
        if Self::avx512_supported() {
            Some(unsafe { Self::new_avx512() })
        } else {
            None
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            HasherInputDyn::Scalar(_) => "scalar",
            HasherInputDyn::Sse2(_) => "sse2",
            HasherInputDyn::Bmi1(_) => "bmi1",
            HasherInputDyn::Avx512(_) => "avx512",
        }
    }

    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        match self {
            HasherInputDyn::Scalar(h) => h.update(data),
            HasherInputDyn::Sse2(h) => h.update(data),
            HasherInputDyn::Bmi1(h) => h.update(data),
            HasherInputDyn::Avx512(h) => h.update(data),
        }
    }

    #[inline]
    pub fn get_block(&mut self, zero_pad: u64) -> BlockHash {
        match self {
            HasherInputDyn::Scalar(h) => h.get_block(zero_pad),
            HasherInputDyn::Sse2(h) => h.get_block(zero_pad),
            HasherInputDyn::Bmi1(h) => h.get_block(zero_pad),
            HasherInputDyn::Avx512(h) => h.get_block(zero_pad),
        }
    }

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

#[cfg(target_arch = "x86_64")]
fn is_amd_vec_rot_slow() -> bool {
    unsafe {
        use core::arch::x86_64::__cpuid;
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
        let eax = __cpuid(1).eax;
        let base_family = ((eax >> 8) & 0xF) as u32;
        let ext_family = ((eax >> 20) & 0xFF) as u32;
        let family = base_family + ext_family;
        matches!(family, 0x19 | 0x1A)
    }
}

// ============================================================================
// Shared Implementations
// ============================================================================

impl Default for HasherInputDyn {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_a_backend() {
        let h = HasherInputDyn::new();
        let name = h.backend_name();
        #[cfg(target_arch = "x86_64")]
        assert!(matches!(name, "scalar" | "sse2" | "bmi1" | "avx512"));
        #[cfg(target_arch = "aarch64")]
        assert!(matches!(name, "scalar")); // or "neon" when ported
    }

    #[test]
    fn dyn_matches_scalar() {
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
        assert_eq!(a.end(), b.end());
    }

    #[test]
    fn zero_pad_behavior() {
        let block_size = 64usize;
        let mut data = vec![0u8; 100];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(13);
        }

        let mut h = HasherInputDyn::new();
        h.update(&data[..block_size]);
        let _b0 = h.get_block(0);
        h.update(&data[block_size..]);
        let _b1 = h.get_block((block_size - (data.len() - block_size)) as u64);
        let file_md5 = h.end();

        use md5::{Digest, Md5};
        let mut m = Md5::new();
        m.update(&data);
        let expected = m.finalize();
        assert_eq!(&file_md5[..], &expected[..]);
    }
}

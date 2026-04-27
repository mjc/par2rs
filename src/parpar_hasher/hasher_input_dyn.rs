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
//! Selection rules match upstream's Zen3-relevant branches (see
//! `hasher.cpp:105-117`):
//!
//! * `bmi1` available → `Bmi1` backend (preferred on big cores).
//! * else → `Scalar` backend (always-available baseline).
//!
//! `Sse2` is not selected automatically: on Zen3 it is slower than
//! `Scalar` (see `benches/parpar_hasher_input.rs`), and upstream only
//! picks SSE on small cores (Tremont / Gracemont / Zen1) which the
//! current target audience (5950X) doesn't include. The variant is
//! kept reachable for future small-core dispatch and as a correctness
//! oracle for the scalar/BMI1 paths.

use super::hasher_input::{BlockHash, HasherInput};
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
}

impl HasherInputDyn {
    /// Pick the best backend for the current CPU and construct it.
    /// Mirrors upstream `HasherInput_Create()`.
    pub fn new() -> Self {
        if is_x86_feature_detected!("bmi1") {
            HasherInputDyn::Bmi1(HasherInput::new())
        } else {
            HasherInputDyn::Scalar(HasherInput::new())
        }
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

    /// Identifies which backend was selected. Mostly for diagnostics.
    pub fn backend_name(&self) -> &'static str {
        match self {
            HasherInputDyn::Scalar(_) => "scalar",
            HasherInputDyn::Sse2(_) => "sse2",
            HasherInputDyn::Bmi1(_) => "bmi1",
        }
    }

    /// Feed bytes. See [`HasherInput::update`].
    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        match self {
            HasherInputDyn::Scalar(h) => h.update(data),
            HasherInputDyn::Sse2(h) => h.update(data),
            HasherInputDyn::Bmi1(h) => h.update(data),
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
        }
    }
}

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
        // Just ensure construction works.
        let name = h.backend_name();
        assert!(matches!(name, "scalar" | "sse2" | "bmi1"));
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

//! Rust FFI wrappers around the embedded ParPar hasher sources.
//!
//! This module is only built on x86_64 when the `parpar-compare` feature is enabled.

use crate::domain::Md5Hash;
use std::ffi::c_void;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HasherInputMethod {
    Scalar = 0,
    Simd = 1,
    Crc = 2,
    SimdCrc = 3,
    Bmi1 = 4,
    Avx512 = 5,
}

unsafe extern "C" {
    fn parpar_md5single_new() -> *mut c_void;
    fn parpar_md5single_update(ctx: *mut c_void, data: *const u8, len: usize);
    fn parpar_md5single_end(ctx: *mut c_void, out: *mut u8);
    fn parpar_md5single_free(ctx: *mut c_void);
    fn parpar_md5single_hash(data: *const u8, len: usize, out: *mut u8);

    fn parpar_hasher_input_new(method: u32) -> *mut c_void;
    fn parpar_hasher_input_update(ctx: *mut c_void, data: *const u8, len: usize);
    fn parpar_hasher_input_end(ctx: *mut c_void, out: *mut u8);
    fn parpar_hasher_input_reset(ctx: *mut c_void);
    fn parpar_hasher_input_free(ctx: *mut c_void);
    fn parpar_hasher_input_hash(method: u32, data: *const u8, len: usize, out: *mut u8);
    fn parpar_hasher_input_is_available(method: u32) -> bool;

    fn parpar_crc32_compute(data: *const u8, len: usize) -> u32;
}

impl HasherInputMethod {
    pub fn is_available(self) -> bool {
        unsafe { parpar_hasher_input_is_available(self as u32) }
    }
}

pub mod md5 {
    use super::*;

    pub struct ParParMd5 {
        ctx: *mut c_void,
    }

    impl ParParMd5 {
        pub fn new() -> Option<Self> {
            let ctx = unsafe { parpar_md5single_new() };
            (!ctx.is_null()).then_some(Self { ctx })
        }

        pub fn update(&mut self, data: &[u8]) {
            unsafe { parpar_md5single_update(self.ctx, data.as_ptr(), data.len()) }
        }

        pub fn finalize(mut self) -> Md5Hash {
            let mut digest = [0u8; 16];
            let ctx = self.ctx;
            self.ctx = std::ptr::null_mut();
            unsafe {
                parpar_md5single_end(ctx, digest.as_mut_ptr());
                parpar_md5single_free(ctx);
            }
            Md5Hash::new(digest)
        }
    }

    impl Drop for ParParMd5 {
        fn drop(&mut self) {
            if !self.ctx.is_null() {
                unsafe { parpar_md5single_free(self.ctx) };
                self.ctx = std::ptr::null_mut();
            }
        }
    }

    pub fn md5_hash(data: &[u8]) -> Md5Hash {
        let mut digest = [0u8; 16];
        unsafe {
            parpar_md5single_hash(data.as_ptr(), data.len(), digest.as_mut_ptr());
        }
        Md5Hash::new(digest)
    }
}

pub mod hasher_input {
    use super::*;

    pub struct ParParHasherInput {
        ctx: *mut c_void,
    }

    impl ParParHasherInput {
        pub fn new(method: HasherInputMethod) -> Option<Self> {
            let ctx = unsafe { parpar_hasher_input_new(method as u32) };
            (!ctx.is_null()).then_some(Self { ctx })
        }

        pub fn update(&mut self, data: &[u8]) {
            unsafe { parpar_hasher_input_update(self.ctx, data.as_ptr(), data.len()) }
        }

        pub fn reset(&mut self) {
            unsafe { parpar_hasher_input_reset(self.ctx) }
        }

        pub fn finalize(mut self) -> Md5Hash {
            let mut digest = [0u8; 16];
            let ctx = self.ctx;
            self.ctx = std::ptr::null_mut();
            unsafe {
                parpar_hasher_input_end(ctx, digest.as_mut_ptr());
                parpar_hasher_input_free(ctx);
            }
            Md5Hash::new(digest)
        }

        pub fn hash(method: HasherInputMethod, data: &[u8]) -> Md5Hash {
            let mut digest = [0u8; 16];
            unsafe {
                parpar_hasher_input_hash(
                    method as u32,
                    data.as_ptr(),
                    data.len(),
                    digest.as_mut_ptr(),
                );
            }
            Md5Hash::new(digest)
        }
    }

    impl Drop for ParParHasherInput {
        fn drop(&mut self) {
            if !self.ctx.is_null() {
                unsafe { parpar_hasher_input_free(self.ctx) };
                self.ctx = std::ptr::null_mut();
            }
        }
    }
}

pub mod crc32 {
    use super::*;

    pub fn crc32_compute(data: &[u8]) -> u32 {
        unsafe { parpar_crc32_compute(data.as_ptr(), data.len()) }
    }
}

pub use crc32::crc32_compute;
pub use hasher_input::ParParHasherInput;
pub use md5::ParParMd5;

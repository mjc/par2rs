//! 32-byte aligned buffer allocation for SIMD operations.

/// Allocate a Vec<u8> with 32-byte alignment for optimal SIMD performance
///
/// AVX2 aligned stores (`_mm256_store_si256`) are faster than unaligned stores,
/// especially on pre-Sandy Bridge CPUs where unaligned operations have ~5-10 cycle penalty.
///
/// This function allocates a zeroed buffer with the specified size, aligned to
/// 32-byte boundaries for use with AVX2 SIMD operations.
///
/// # Arguments
/// * `size` - Number of bytes to allocate
///
/// # Returns
/// A `Vec<u8>` with the data pointer aligned to 32 bytes
///
/// # Panics
/// Panics if allocation fails
#[inline]
pub fn alloc_aligned_vec(size: usize) -> Vec<u8> {
    let layout = std::alloc::Layout::from_size_align(size, 32).expect("Invalid layout");
    unsafe {
        let ptr = std::alloc::alloc_zeroed(layout);
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Vec::from_raw_parts(ptr, size, size)
    }
}

/// Owned 32-byte aligned byte buffer.
///
/// This is used by create-side hot paths where capacity changes must be explicit:
/// the buffer has a fixed length and never grows via `push` or `extend`.
pub struct AlignedVec {
    ptr: std::ptr::NonNull<u8>,
    len: usize,
}

impl AlignedVec {
    #[inline]
    pub fn new_zeroed(len: usize) -> Self {
        if len == 0 {
            return Self {
                ptr: std::ptr::NonNull::dangling(),
                len,
            };
        }

        let layout = Self::layout(len);
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        Self {
            ptr: unsafe { std::ptr::NonNull::new_unchecked(ptr) },
            len,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    #[inline]
    pub fn fill(&mut self, value: u8) {
        self.as_mut_slice().fill(value);
    }

    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn resize_zeroed(&mut self, len: usize) {
        if self.len == len {
            self.fill(0);
            return;
        }

        *self = Self::new_zeroed(len);
    }

    #[inline]
    fn layout(len: usize) -> std::alloc::Layout {
        std::alloc::Layout::from_size_align(len, 32).expect("invalid aligned buffer layout")
    }
}

impl std::ops::Deref for AlignedVec {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl std::ops::DerefMut for AlignedVec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl Drop for AlignedVec {
    fn drop(&mut self) {
        if self.len != 0 {
            unsafe {
                std::alloc::dealloc(self.ptr.as_ptr(), Self::layout(self.len));
            }
        }
    }
}

unsafe impl Send for AlignedVec {}
unsafe impl Sync for AlignedVec {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_aligned_vec() {
        let vec = alloc_aligned_vec(256);
        assert_eq!(vec.len(), 256);
        assert_eq!(vec.as_ptr() as usize % 32, 0, "Should be 32-byte aligned");
    }

    #[test]
    fn test_multiple_allocations() {
        for size in [32, 64, 128, 256, 1024, 8192] {
            let vec = alloc_aligned_vec(size);
            assert_eq!(vec.len(), size);
            assert_eq!(vec.as_ptr() as usize % 32, 0, "Size {} not aligned", size);
        }
    }

    #[test]
    fn test_alignment_maintained() {
        let vec = alloc_aligned_vec(1024);
        let ptr = vec.as_ptr() as usize;
        assert_eq!(ptr % 32, 0);

        // Verify zeros
        assert!(vec.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_aligned_vec_alignment_and_resize() {
        let mut vec = AlignedVec::new_zeroed(1024);
        assert_eq!(vec.len(), 1024);
        assert_eq!(vec.as_ptr() as usize % 32, 0);
        assert!(vec.iter().all(|&b| b == 0));

        vec[0] = 9;
        vec.resize_zeroed(256);
        assert_eq!(vec.len(), 256);
        assert_eq!(vec.as_ptr() as usize % 32, 0);
        assert!(vec.iter().all(|&b| b == 0));
    }
}

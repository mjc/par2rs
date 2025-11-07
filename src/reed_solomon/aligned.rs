//! 32-byte aligned buffer allocation for SIMD operations
//!
//! Provides a simple function to allocate Vec<u8> with 32-byte alignment
//! for optimal AVX2 SIMD performance.

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
}

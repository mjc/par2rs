# Reed-Solomon SIMD Refactoring Opportunities

This document outlines potential refactoring opportunities identified in the Reed-Solomon SIMD implementation after completing the PSHUFB, NEON, and portable_simd optimizations.

## Performance Reality Check

**Critical findings from cross-platform testing:**

1. **x86_64**: portable_simd is **SLOWER than scalar** for this algorithm
   - Only PSHUFB provides speedup
   - Likely due to poor codegen for this specific nibble-lookup pattern

2. **ARM64 (Apple Silicon)**: Both NEON and portable_simd achieve ~2.2x speedup
   - NEON performance confirmed on M1
   - Need testing on non-Apple ARM CPUs to determine if NEON code is necessary

3. **Other platforms**: portable_simd untested, assume it works but may be slow

**Implication**: We cannot use portable_simd as a universal fallback. Platform-specific dispatch is critical.

## 1. Duplicate Nibble Table Building (High Priority)

### Current State

The same nibble table building logic is duplicated **3 times** across the codebase:

1. **`simd_pshufb.rs::build_pshufb_tables()`** (lines 65-86)
2. **`simd_neon.rs::build_neon_tables()`** (lines 22-43)
3. **`simd.rs::process_slice_multiply_add_portable_simd()`** (lines 219-241, inline)

All three implementations do the exact same thing:

- Take a 256-entry `u16` table
- Split it into 4 16-byte nibble lookup tables
- Return as a tuple: `([u8; 16], [u8; 16], [u8; 16], [u8; 16])`

### Impact

- **Code duplication**: ~25 lines duplicated 3 times
- **Maintenance burden**: Changes must be made in 3 places
- **Inconsistency risk**: Easy for implementations to diverge

### Proposed Solution

Create a common nibble table building function:

```rust
// In src/reed_solomon/simd_common.rs or reedsolomon.rs

/// Nibble lookup tables for SIMD operations
/// 
/// Stores the 4 16-byte tables needed for nibble-based GF(2^16) multiplication:
/// - lo_nib_lo_byte: Low nibble → result low byte
/// - lo_nib_hi_byte: Low nibble → result high byte
/// - hi_nib_lo_byte: High nibble → result low byte
/// - hi_nib_hi_byte: High nibble → result high byte
pub struct NibbleTables {
    pub lo_nib_lo_byte: [u8; 16],
    pub lo_nib_hi_byte: [u8; 16],
    pub hi_nib_lo_byte: [u8; 16],
    pub hi_nib_hi_byte: [u8; 16],
}

/// Build nibble lookup tables from a 256-entry GF(2^16) multiplication table
///
/// Splits each byte into low/high nibbles (4 bits each) for SIMD table lookups.
/// This reduces table size from 512 bytes to 64 bytes (8x reduction).
pub fn build_nibble_tables(table: &[u16; 256]) -> NibbleTables {
    let mut lo_nib_lo_byte = [0u8; 16];
    let mut lo_nib_hi_byte = [0u8; 16];
    let mut hi_nib_lo_byte = [0u8; 16];
    let mut hi_nib_hi_byte = [0u8; 16];

    for nib in 0..16 {
        // Low nibble: input byte = nib (0x0N)
        let result_lo = table[nib];
        lo_nib_lo_byte[nib] = (result_lo & 0xFF) as u8;
        lo_nib_hi_byte[nib] = (result_lo >> 8) as u8;

        // High nibble: input byte = nib << 4 (0xN0)
        let result_hi = table[nib << 4];
        hi_nib_lo_byte[nib] = (result_hi & 0xFF) as u8;
        hi_nib_hi_byte[nib] = (result_hi >> 8) as u8;
    }

    NibbleTables {
        lo_nib_lo_byte,
        lo_nib_hi_byte,
        hi_nib_lo_byte,
        hi_nib_hi_byte,
    }
}
```

Then update all three implementations to use it:

- `simd_pshufb.rs`: Replace `build_pshufb_tables()` with `build_nibble_tables()`
- `simd_neon.rs`: Replace `build_neon_tables()` with `build_nibble_tables()`
- `simd.rs`: Replace inline table building with `build_nibble_tables()`

### Benefits

- **Eliminates 50+ lines of duplicate code**
- **Single source of truth** for nibble table building
- **Type safety** with `NibbleTables` struct vs anonymous tuples
- **Better documentation** of what each table represents

## 2. Correct Dispatch Logic (High Priority - From TODO List)

### Current State

The dispatch logic is platform-specific and incomplete:

**`simd.rs::process_slice_multiply_add_simd()`**:

- x86_64 version: Detects AVX2/SSSE3, calls PSHUFB implementation
- Non-x86_64 version: **Does nothing** (just an empty function)
- NEON and portable_simd are **not integrated** into dispatch

**Result**: On ARM64, there's no automatic dispatch to NEON or portable_simd.

### Impact

- **Missing performance** on non-x86_64 platforms
- **Wrong fallback on x86_64**: portable_simd is slower than scalar!
- **Manual selection required** to use NEON/portable_simd
- **Incomplete implementation** of cross-platform SIMD

### Performance-Based Dispatch Strategy

Based on real-world testing:

**x86_64:**

1. Try PSHUFB (AVX2/SSSE3) - **2.76x speedup** ✅
2. Fall back to **scalar** - portable_simd is **slower than scalar** ❌

**ARM64:**

1. Try NEON - **2.2-2.4x speedup** ✅ (confirmed on Apple Silicon)
2. Fall back to portable_simd - **2.2-2.4x speedup** ✅ (confirmed on Apple Silicon)
   - *Note: Need non-Apple ARM testing to determine if NEON is necessary*
3. Fall back to scalar

**Other platforms:**

1. Try portable_simd (unconfirmed performance)
2. Fall back to scalar

### Proposed Solution

Implement platform-aware runtime dispatch:

```rust
/// Detect best available SIMD implementation for current platform
/// 
/// Returns the optimal implementation based on:
/// - Platform architecture (x86_64, aarch64, etc.)
/// - Available CPU features (AVX2, SSSE3, NEON)
/// - Known performance characteristics (some SIMD slower than scalar!)
pub fn detect_best_simd() -> SimdImplementation {
    #[cfg(target_arch = "x86_64")]
    {
        // On x86_64, ONLY use PSHUFB - portable_simd is slower than scalar!
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("ssse3") {
            return SimdImplementation::Pshufb;
        }
        // No PSHUFB? Use scalar - do NOT use portable_simd
        return SimdImplementation::Scalar;
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // NEON is always available on ARM64 and provides good speedup
        // Question: Do we need NEON-specific code or does portable_simd work everywhere?
        // For now, prefer NEON over portable_simd (confirmed faster on Apple Silicon)
        return SimdImplementation::Neon;
        
        // Alternative if NEON and portable_simd are equivalent on all ARM:
        // return SimdImplementation::PortableSimd;
    }
    
    // Other platforms: Try portable_simd (performance unknown)
    // Note: This may be slower than scalar on some platforms!
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        SimdImplementation::PortableSimd
    }
}

/// Unified SIMD dispatch that works correctly on all platforms
/// 
/// Automatically selects the fastest implementation for the current platform.
/// On x86_64, falls back to scalar (NOT portable_simd) when PSHUFB unavailable.
pub fn process_slice_multiply_add_simd_dispatch(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    match detect_best_simd() {
        SimdImplementation::Pshufb => {
            #[cfg(target_arch = "x86_64")]
            unsafe {
                crate::reed_solomon::simd_pshufb::process_slice_multiply_add_pshufb(
                    input, output, tables
                );
            }
        }
        SimdImplementation::Neon => {
            #[cfg(target_arch = "aarch64")]
            unsafe {
                crate::reed_solomon::simd_neon::process_slice_multiply_add_neon(
                    input, output, tables
                );
            }
        }
        SimdImplementation::PortableSimd => {
            // Only used on ARM64 (as fallback) or non-x86_64/non-aarch64 platforms
            // NOT used on x86_64 (slower than scalar!)
            unsafe {
                process_slice_multiply_add_portable_simd(input, output, tables);
            }
        }
        SimdImplementation::Scalar => {
            // Scalar fallback - used on x86_64 without PSHUFB
            process_slice_multiply_add_scalar(input, output, tables);
        }
    }
}
```

### Benefits

- **Correct performance** on all platforms (no slow SIMD on x86_64)
- **Automatic best-path selection** based on real-world testing
- **Full cross-platform support** with appropriate fallbacks
- **Completes TODO item** from the task list
- **Documents performance reality** in code

### Testing Needs

- ✅ x86_64 with AVX2: PSHUFB confirmed fast
- ✅ x86_64 without AVX2: Scalar fallback correct
- ✅ ARM64 (Apple Silicon): NEON confirmed fast, portable_simd also fast
- ⏳ ARM64 (non-Apple): Need to test if NEON code is necessary
- ⏳ Other platforms: Need to test portable_simd performance

## 3. Scalar Fallback Duplication

### Current State

Scalar fallback code exists in multiple places:

1. **`simd_neon.rs::process_scalar()`** (lines 156-178)
   - Full scalar implementation with word-by-word processing
   - Handles odd trailing bytes

2. **`simd.rs::process_slice_multiply_add_portable_simd()`** (lines 310-330)
   - Inline scalar loop for remaining bytes

3. **`simd.rs::process_slice_multiply_add_avx2_unrolled()`**
   - Uses similar scalar pattern for remaining words

### Impact

- **Code duplication**: Similar scalar logic in 3+ places
- **Inconsistent handling**: Different approaches to odd bytes
- **Maintenance burden**: Bug fixes need to be applied multiple times

### Proposed Solution

Extract to a common scalar implementation:

```rust
// In src/reed_solomon/reedsolomon.rs or simd_common.rs

/// Scalar GF(2^16) multiply-add fallback for small buffers or remainder bytes
///
/// Processes input word-by-word using lookup tables.
/// Handles odd trailing bytes correctly.
pub fn process_slice_multiply_add_scalar(
    input: &[u8],
    output: &mut [u8],
    tables: &SplitMulTable,
) {
    let len = input.len().min(output.len());
    
    // SAFETY: We're reinterpreting bytes as u16. This is safe because:
    // - x86_64/ARM64 support unaligned loads
    // - We've checked we have enough bytes
    let in_words = unsafe {
        std::slice::from_raw_parts(input.as_ptr() as *const u16, len / 2)
    };
    let out_words = unsafe {
        std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u16, len / 2)
    };
    
    for i in 0..in_words.len() {
        let in_word = in_words[i];
        let out_word = out_words[i];
        let result = tables.low[(in_word & 0xFF) as usize] 
                   ^ tables.high[(in_word >> 8) as usize];
        out_words[i] = out_word ^ result;
    }

    // Handle odd trailing byte
    if len % 2 == 1 {
        let last_idx = len - 1;
        let in_byte = input[last_idx];
        let out_byte = output[last_idx];
        let result_low = tables.low[in_byte as usize];
        output[last_idx] = out_byte ^ (result_low & 0xFF) as u8;
    }
}
```

Then use it in all SIMD implementations for:

- Small buffer fallback (< minimum SIMD size)
- Remainder bytes after SIMD processing

### Benefits

- **Single implementation** of scalar fallback
- **Consistent behavior** across all SIMD variants
- **Easier to optimize** - one place to improve scalar performance
- **Reduced test surface** - test scalar once, not 3+ times

## 4. Inconsistent Small Buffer Handling

### Current State

Each SIMD implementation handles small buffers differently:

- **PSHUFB** (`simd_pshufb.rs`):
  - Returns early if `len < 32`
  - Caller must handle small buffers
  
- **NEON** (`simd_neon.rs`):
  - Falls back to `process_scalar()` if `len < 16`
  - Self-contained handling
  
- **portable_simd** (`simd.rs`):
  - Processes what it can with SIMD (16-byte chunks)
  - Uses inline scalar loop for remainder
  - No early return

### Impact

- **Inconsistent API behavior** across implementations
- **Performance cliffs** at different buffer sizes
- **Unclear contracts** - does caller handle small buffers or not?

### Proposed Solution

Standardize small buffer handling with two approaches:

**Option A: Self-Contained (Recommended)**
Each SIMD function handles all buffer sizes internally:
```rust
pub fn process_slice_multiply_add_neon(...) {
    if len < 16 {
        // Fall back to scalar for small buffers
        process_slice_multiply_add_scalar(input, output, tables);
        return;
    }
    
    // SIMD processing...
    
    // Scalar fallback for remainder
    if idx < len {
        process_slice_multiply_add_scalar(&input[idx..], &mut output[idx..], tables);
    }
}
```

**Option B: Caller-Handled**
Document that functions require minimum buffer sizes:

```rust
/// # Requirements
/// - `input` and `output` must be at least 16 bytes for optimal performance
/// - For buffers < 16 bytes, use `process_slice_multiply_add_scalar()` instead
pub fn process_slice_multiply_add_neon(...) {
    debug_assert!(input.len() >= 16, "Buffer too small for SIMD");
    // ...
}
```

**Recommendation**: Use Option A (self-contained). Benefits:

- Simpler API for callers
- No need to check buffer size before calling
- Graceful degradation for small buffers

### Benefits

- **Consistent API** across all SIMD implementations
- **Predictable performance** characteristics
- **Easier to use** - callers don't need size checks
- **Better encapsulation** of SIMD vs scalar decision

## 5. Module Organization

### Current State

SIMD code is spread across multiple files:

- `simd.rs` - Dispatch, portable_simd, AVX2 unrolled
- `simd_pshufb.rs` - PSHUFB implementation
- `simd_neon.rs` - NEON implementation
- No `simd_common.rs` or shared utilities

### Proposed Solution

Consider reorganizing:

```
src/reed_solomon/
├── simd/
│   ├── mod.rs           # Public API, dispatch logic
│   ├── common.rs        # Shared: NibbleTables, scalar fallback
│   ├── pshufb.rs        # x86_64 PSHUFB implementation
│   ├── neon.rs          # ARM64 NEON implementation
│   └── portable.rs      # Cross-platform portable_simd
```

### Benefits

- **Clearer separation** of concerns
- **Easier to find** shared code
- **Better namespace** organization
- **Simpler imports** for common utilities

## Implementation Priority

**UPDATED based on x86_64 testing results:**

Suggested order for implementing these refactorings:

1. **#2: Correct dispatch logic** (2-3 hours) - **DO THIS FIRST**
   - **Critical**: Prevents using slow portable_simd on x86_64
   - Medium risk, touches dispatch paths
   - Completes cross-platform support correctly
   - **Resolves TODO item**
   - **Fixes performance regression** on x86_64 without PSHUFB

2. **#3: Extract scalar fallback** (1-2 hours)
   - Low risk, high value
   - Makes other refactorings easier
   - Needed for correct dispatch fallback

3. **#1: Common nibble table building** (1-2 hours)
   - Low risk, removes most duplication
   - Good cleanup after dispatch is fixed

4. **#4: Standardize small buffer handling** (2-3 hours)
   - Medium risk, requires updating all implementations
   - Improves API consistency
   - Lower priority now that dispatch is correct

5. **#5: Module reorganization** (2-3 hours)
   - Low risk if done carefully
   - Quality of life improvement
   - Optional - can be done later

**Key insight**: Fixing dispatch (#2) is now TOP PRIORITY because portable_simd is slower than scalar on x86_64. We must not use it there!

## Testing Strategy

For each refactoring:

1. Ensure all existing tests pass
2. Add tests for new common functions
3. Verify performance with benchmarks (no regression)
4. Test on both x86_64 and ARM64 platforms

## Performance Impact

All proposed refactorings are **structural changes** with no expected performance impact:

- Extracting functions: Zero overhead with inlining
- Using structs vs tuples: Zero-cost abstraction
- Improving dispatch: May improve performance by choosing best path

Benchmark before/after to verify no regression.

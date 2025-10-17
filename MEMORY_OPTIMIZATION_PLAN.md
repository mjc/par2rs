# Memory Optimization Plan for PAR2 Recovery Slice Loading

## Problem
Currently loading ~1.9GB into memory for large PAR2 sets when it should use <128MB.
The issue: `RecoverySlicePacket` loads all `recovery_data` into Vec<u8> immediately.

## Solution: Incremental Lazy Loading Implementation

### Phase 1: Add Metadata Infrastructure (Non-Breaking) ✓
- ✓ Create `RecoverySliceMetadata` struct 
- ✓ Add `load_data()` method for on-demand loading
- ✓ Add comprehensive tests (all passing)
- ✓ Keep RecoverySlicePacket for now (backward compatibility)

### Phase 2: Add Dual Parsing Support (Non-Breaking)
- Add `parse_recovery_metadata()` function to file_ops.rs
- Return both packets AND metadata from loading functions
- Update callers to use metadata when available, fall back to packets
- All tests still pass, build still works

### Phase 3: Update Repair Module (Non-Breaking)
- Modify RecoverySetInfo to store Vec<RecoverySliceMetadata> alongside Vec<RecoverySlicePacket>
- Update repair logic to prefer metadata over packets
- Add flag to control which approach is used
- Verify memory savings with tests

### Phase 4: Update RecoverySliceProvider (Non-Breaking)
- Add method to accept metadata instead of full packets
- Load data on-demand from metadata.load_data()
- Keep existing packet-based method working
- Measure and verify memory usage

### Phase 5: Switch Default Behavior (Non-Breaking)
- Change default to use metadata-only approach
- Keep packet-based approach as fallback
- Add memory usage logging
- All tests still pass

### Phase 6: Remove Old Code (Breaking but Safe)
- Remove RecoverySlicePacket's recovery_data field (make it optional/empty)
- Remove packet-based codepaths
- Clean up any remaining references
- Final testing and validation

## Expected Memory Savings
- Before: ~1.9GB for large PAR2 set (all recovery data loaded)
- After: <128MB (only metadata + buffers for active reconstruction)
- Savings: ~93% reduction

## Key Principles
- Each phase keeps the build working
- Tests pass at every step
- Easy to revert if something goes wrong
- Gradual transition minimizes risk

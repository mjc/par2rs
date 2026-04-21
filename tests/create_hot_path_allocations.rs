#![cfg(not(feature = "nix-build"))]

use par2rs::create::{CreateContextBuilder, SilentCreateReporter};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct CountingAllocator;

static ALLOCATIONS_ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCATION_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ALLOCATIONS_ENABLED.load(Ordering::Relaxed) {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ALLOCATIONS_ENABLED.load(Ordering::Relaxed) {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ALLOCATIONS_ENABLED.load(Ordering::Relaxed) {
            ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn count_allocations_during_create(
    source_path: &Path,
    output_path: &Path,
    memory_limit: Option<usize>,
) -> usize {
    let mut builder = CreateContextBuilder::new()
        .output_name(output_path.to_string_lossy().to_string())
        .source_files(vec![source_path.to_path_buf()])
        .block_size(1024)
        .recovery_block_count(2)
        .thread_count(1)
        .reporter(Box::new(SilentCreateReporter));

    if let Some(limit) = memory_limit {
        builder = builder.memory_limit(limit);
    }

    let mut context = builder.build().unwrap();

    ALLOCATION_COUNT.store(0, Ordering::Relaxed);
    ALLOCATIONS_ENABLED.store(true, Ordering::SeqCst);
    context.create().unwrap();
    ALLOCATIONS_ENABLED.store(false, Ordering::SeqCst);

    ALLOCATION_COUNT.load(Ordering::Relaxed)
}

#[test]
fn create_hot_path_allocations_do_not_scale_with_chunk_count() {
    let tmp = tempfile::tempdir().unwrap();
    let source_path = tmp.path().join("source.bin");
    let data: Vec<u8> = (0..4096).map(|i| (i % 251) as u8).collect();
    std::fs::write(&source_path, data).unwrap();

    let one_chunk =
        count_allocations_during_create(&source_path, &tmp.path().join("one.par2"), None);
    let multi_chunk =
        count_allocations_during_create(&source_path, &tmp.path().join("multi.par2"), Some(24));

    assert!(
        one_chunk < 500,
        "single-chunk create allocated too much: {one_chunk}"
    );
    assert!(
        multi_chunk <= one_chunk + 20,
        "multi-chunk create allocations scaled with chunk count: one={one_chunk}, multi={multi_chunk}"
    );
}

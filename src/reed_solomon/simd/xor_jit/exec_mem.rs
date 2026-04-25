#![allow(dead_code)]

use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

pub struct ExecutableBuffer {
    ptr: NonNull<u8>,
    capacity: usize,
    len: usize,
    executable: bool,
}

impl ExecutableBuffer {
    pub fn new(capacity: usize) -> io::Result<Self> {
        let page_size = page_size();
        let capacity = capacity.max(1).next_multiple_of(page_size);
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let ptr = NonNull::new(ptr.cast::<u8>())
            .ok_or_else(|| io::Error::other("mmap returned null executable buffer"))?;

        Ok(Self {
            ptr,
            capacity,
            len: 0,
            executable: false,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds executable buffer capacity",
            ));
        }

        if self.executable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "executable buffer has already been sealed",
            ));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.as_ptr(), bytes.len());
        }
        self.len = bytes.len();
        self.make_executable()
    }

    pub unsafe fn function<F: Copy>(&self) -> F {
        debug_assert!(self.executable);
        debug_assert!(self.len > 0);
        std::mem::transmute_copy(&self.ptr)
    }

    fn make_executable(&mut self) -> io::Result<()> {
        let result = unsafe {
            libc::mprotect(
                self.ptr.as_ptr().cast::<c_void>(),
                self.capacity,
                libc::PROT_READ | libc::PROT_EXEC,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        self.executable = true;
        Ok(())
    }
}

impl Drop for ExecutableBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast::<c_void>(), self.capacity);
        }
    }
}

fn page_size() -> usize {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        4096
    } else {
        value as usize
    }
}

#![allow(dead_code)]

use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

pub struct ExecutableBuffer {
    ptr: NonNull<u8>,
    capacity: usize,
    len: usize,
    protection: Protection,
}

pub struct MutableExecutableBuffer {
    ptr: NonNull<u8>,
    capacity: usize,
    len: usize,
}

impl ExecutableBuffer {
    pub fn new(capacity: usize) -> io::Result<Self> {
        let capacity = round_to_page_size(capacity.max(1));
        Ok(Self {
            ptr: map_writable(capacity)?,
            capacity,
            len: 0,
            protection: Protection::Writable,
        })
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds executable buffer capacity",
            ));
        }

        if self.protection == Protection::Executable {
            set_protection(self.ptr, self.capacity, Protection::Writable)?;
            self.protection = Protection::Writable;
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.as_ptr(), bytes.len());
        }
        self.len = bytes.len();
        self.make_executable()
    }

    pub unsafe fn function<F: Copy>(&self) -> F {
        debug_assert_eq!(self.protection, Protection::Executable);
        debug_assert!(self.len > 0);
        function_from_ptr(self.ptr)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    fn make_executable(&mut self) -> io::Result<()> {
        set_protection(self.ptr, self.capacity, Protection::Executable)?;
        self.protection = Protection::Executable;
        Ok(())
    }
}

impl MutableExecutableBuffer {
    pub fn new(capacity: usize) -> io::Result<Self> {
        let capacity = round_to_page_size(capacity.max(1));
        Ok(Self {
            ptr: map_writable_executable(capacity)?,
            capacity,
            len: 0,
        })
    }

    pub fn overwrite(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds mutable executable buffer capacity",
            ));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.as_ptr(), bytes.len());
        }
        self.len = bytes.len();
        Ok(())
    }

    pub fn overwrite_at(&mut self, offset: usize, bytes: &[u8]) -> io::Result<()> {
        if offset
            .checked_add(bytes.len())
            .is_none_or(|len| len > self.capacity)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds mutable executable buffer capacity",
            ));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.ptr.as_ptr().add(offset),
                bytes.len(),
            );
        }
        self.len = self.len.max(offset + bytes.len());
        Ok(())
    }

    pub fn append_byte(&mut self, byte: u8) -> io::Result<()> {
        if self.len >= self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds mutable executable buffer capacity",
            ));
        }

        unsafe {
            self.ptr.as_ptr().add(self.len).write(byte);
        }
        self.len += 1;
        Ok(())
    }

    pub fn append_bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        let end = self.len.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds mutable executable buffer capacity",
            )
        })?;
        if end > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "generated code exceeds mutable executable buffer capacity",
            ));
        }

        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.ptr.as_ptr().add(self.len),
                bytes.len(),
            );
        }
        self.len = end;
        Ok(())
    }

    pub fn clear_cacheline_bytes_at(&mut self, offset: usize, len: usize) -> io::Result<()> {
        if offset
            .checked_add(len)
            .is_none_or(|end| end > self.capacity)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "clear range exceeds mutable executable buffer capacity",
            ));
        }

        for index in (0..len).step_by(64) {
            unsafe {
                self.ptr.as_ptr().add(offset + index).write(0);
            }
        }
        self.len = self.len.max(offset + len);
        Ok(())
    }

    pub fn set_len_for_overwrite(&mut self, len: usize) -> io::Result<()> {
        if len > self.capacity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mutable executable buffer length exceeds capacity",
            ));
        }

        self.len = len;
        Ok(())
    }

    pub unsafe fn function<F: Copy>(&self) -> F {
        debug_assert!(self.len > 0);
        function_from_ptr(self.ptr)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn copy_prefix(&self, len: usize) -> io::Result<Vec<u8>> {
        if len > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "copy range exceeds mutable executable buffer length",
            ));
        }

        let mut bytes = vec![0; len];
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr.as_ptr(), bytes.as_mut_ptr(), len);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protection {
    Writable,
    Executable,
}

impl Protection {
    const fn flags(self) -> i32 {
        match self {
            Self::Writable => libc::PROT_READ | libc::PROT_WRITE,
            Self::Executable => libc::PROT_READ | libc::PROT_EXEC,
        }
    }
}

impl Drop for ExecutableBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast::<c_void>(), self.capacity);
        }
    }
}

impl Drop for MutableExecutableBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast::<c_void>(), self.capacity);
        }
    }
}

fn map_writable(capacity: usize) -> io::Result<NonNull<u8>> {
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

    NonNull::new(ptr.cast::<u8>())
        .ok_or_else(|| io::Error::other("mmap returned null executable buffer"))
}

fn map_writable_executable(capacity: usize) -> io::Result<NonNull<u8>> {
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            capacity,
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };

    if ptr == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }

    NonNull::new(ptr.cast::<u8>())
        .ok_or_else(|| io::Error::other("mmap returned null mutable executable buffer"))
}

fn set_protection(ptr: NonNull<u8>, capacity: usize, protection: Protection) -> io::Result<()> {
    let result =
        unsafe { libc::mprotect(ptr.as_ptr().cast::<c_void>(), capacity, protection.flags()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

unsafe fn function_from_ptr<F: Copy>(ptr: NonNull<u8>) -> F {
    std::mem::transmute_copy(&ptr)
}

fn round_to_page_size(value: usize) -> usize {
    value.next_multiple_of(page_size())
}

fn page_size() -> usize {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if value <= 0 {
        4096
    } else {
        value as usize
    }
}

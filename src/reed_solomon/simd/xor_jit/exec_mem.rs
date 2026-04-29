#![allow(dead_code)]

use std::ffi::c_void;
use std::ffi::CString;
use std::io;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct ExecutableBuffer {
    ptr: NonNull<u8>,
    capacity: usize,
    len: usize,
    protection: Protection,
}

pub struct MutableExecutableBuffer {
    write_ptr: NonNull<u8>,
    exec_ptr: NonNull<u8>,
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
        let (write_ptr, exec_ptr) = map_writable_executable_pair(capacity)?;
        Ok(Self {
            write_ptr,
            exec_ptr,
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
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.write_ptr.as_ptr(), bytes.len());
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
                self.write_ptr.as_ptr().add(offset),
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
            self.write_ptr.as_ptr().add(self.len).write(byte);
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
                self.write_ptr.as_ptr().add(self.len),
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
                self.write_ptr.as_ptr().add(offset + index).write(0);
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
        function_from_ptr(self.exec_ptr)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.exec_ptr.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.write_ptr.as_ptr()
    }

    pub fn writable_ptr(&self) -> *const u8 {
        self.write_ptr.as_ptr()
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
            std::ptr::copy_nonoverlapping(self.write_ptr.as_ptr(), bytes.as_mut_ptr(), len);
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
            if self.write_ptr != self.exec_ptr {
                libc::munmap(self.exec_ptr.as_ptr().cast::<c_void>(), self.capacity);
            }
            libc::munmap(self.write_ptr.as_ptr().cast::<c_void>(), self.capacity);
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

fn map_writable_executable_pair(capacity: usize) -> io::Result<(NonNull<u8>, NonNull<u8>)> {
    if let Ok(ptr) = map_writable_executable(capacity) {
        if (ptr.as_ptr() as usize) & 63 == 0 {
            return Ok((ptr, ptr));
        }
        unsafe {
            libc::munmap(ptr.as_ptr().cast::<c_void>(), capacity);
        }
    }

    map_dual_writable_executable(capacity)
}

fn map_dual_writable_executable(capacity: usize) -> io::Result<(NonNull<u8>, NonNull<u8>)> {
    static SHM_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let pid = std::process::id();
    let unique = SHM_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = CString::new(format!("/par2rs_xorjit_shm_{pid}_{unique}"))
        .map_err(|_| io::Error::other("invalid shm path"))?;
    let fd = unsafe {
        libc::shm_open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o700,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }

    unsafe {
        libc::shm_unlink(path.as_ptr());
    }

    let result = (|| {
        if unsafe { libc::ftruncate(fd, capacity as libc::off_t) } != 0 {
            return Err(io::Error::last_os_error());
        }

        let write_map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if write_map == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let exec_map = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_EXEC,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if exec_map == libc::MAP_FAILED {
            unsafe {
                libc::munmap(write_map, capacity);
            }
            return Err(io::Error::last_os_error());
        }

        let write_ptr = NonNull::new(write_map.cast::<u8>())
            .ok_or_else(|| io::Error::other("mmap returned null writable alias"))?;
        let exec_ptr = NonNull::new(exec_map.cast::<u8>())
            .ok_or_else(|| io::Error::other("mmap returned null executable alias"))?;
        if ((write_ptr.as_ptr() as usize) & 63) != 0 || ((exec_ptr.as_ptr() as usize) & 63) != 0 {
            unsafe {
                libc::munmap(exec_ptr.as_ptr().cast::<c_void>(), capacity);
                libc::munmap(write_ptr.as_ptr().cast::<c_void>(), capacity);
            }
            return Err(io::Error::other(
                "dual-mapped xor-jit buffer is not cacheline aligned",
            ));
        }

        Ok((write_ptr, exec_ptr))
    })();

    unsafe {
        libc::close(fd);
    }
    result
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

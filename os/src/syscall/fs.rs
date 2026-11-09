//! File and filesystem-related syscalls
use crate::config::PAGE_SIZE;
use crate::fs::{link_file, open_file, unlink_file, OpenFlags, Stat};
use crate::mm::{translated_byte_buffer, PTEFlags, PageTable, UserBuffer, VirtAddr};
use crate::task::{current_task, current_user_token};
use alloc::string::String;
use alloc::vec::Vec;

fn user_path(ptr: *const u8) -> Option<String> {
    let table = PageTable::from_token(current_user_token());
    let mut bytes = Vec::new();
    for offset in 0..=27 {
        let address = (ptr as usize).checked_add(offset)?;
        let va = VirtAddr::from(address);
        if usize::from(va) != address {
            return None;
        }
        let pte = table.translate(va.floor())?;
        if !pte
            .flags()
            .contains(PTEFlags::V | PTEFlags::U | PTEFlags::R)
        {
            return None;
        }
        let byte = pte.ppn().get_bytes_array()[va.page_offset()];
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let path = match user_path(path) {
        Some(path) => path,
        None => return -1,
    };
    let flags = match OpenFlags::from_bits(flags) {
        Some(flags) => flags,
        None => return -1,
    };
    if let Some(inode) = open_file(path.as_str(), flags) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// Copy an opened file's metadata to user memory, including across page boundaries.
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let task = current_task().unwrap();
    let file = {
        let inner = task.inner_exclusive_access();
        match inner.fd_table.get(fd).and_then(|file| file.as_ref()) {
            Some(file) => file.clone(),
            None => return -1,
        }
    };
    let stat = match file.stat() {
        Some(stat) => stat,
        None => return -1,
    };
    let token = current_user_token();
    let table = PageTable::from_token(token);
    let len = core::mem::size_of::<Stat>();
    let end = match (st as usize).checked_add(len) {
        Some(end) => end,
        None => return -1,
    };
    let mut address = st as usize;
    while address < end {
        let va = VirtAddr::from(address);
        if usize::from(va) != address {
            return -1;
        }
        match table.translate(va.floor()) {
            Some(pte)
                if pte
                    .flags()
                    .contains(PTEFlags::V | PTEFlags::U | PTEFlags::W) => {}
            _ => return -1,
        }
        address += (PAGE_SIZE - va.page_offset()).min(end - address);
    }
    let bytes = unsafe { core::slice::from_raw_parts(&stat as *const Stat as *const u8, len) };
    let mut offset = 0;
    for buffer in translated_byte_buffer(token, st as *const u8, len) {
        let end = offset + buffer.len();
        buffer.copy_from_slice(&bytes[offset..end]);
        offset = end;
    }
    0
}

/// Create a hard link in the root directory.
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    match (user_path(old_name), user_path(new_name)) {
        (Some(old_name), Some(new_name)) if link_file(&old_name, &new_name) => 0,
        _ => -1,
    }
}

/// Remove a hard link from the root directory.
pub fn sys_unlinkat(name: *const u8) -> isize {
    match user_path(name) {
        Some(name) if unlink_file(&name) => 0,
        _ => -1,
    }
}

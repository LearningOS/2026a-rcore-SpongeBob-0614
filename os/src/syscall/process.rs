//! Process management syscalls
//!
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE,
    fs::{open_file, OpenFlags},
    mm::{
        translated_byte_buffer, translated_refmut, translated_str, MapPermission, VirtAddr,
        VirtPageNum,
    },
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, map_current_pages,
        suspend_current_and_run_next, unmap_current_pages, TaskControlBlock,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// Write the timer value to user memory, including when TimeVal spans two pages.
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let _t = get_time_us();
    let _time_val = TimeVal {
        sec: _t / 1_000_000,
        usec: _t % 1_000_000,
    };
    let buffers = translated_byte_buffer(
        current_user_token(),
        _ts as *const u8,
        core::mem::size_of::<TimeVal>(),
    );
    let ptr = &_time_val;
    let src: &[u8] = unsafe {
        core::slice::from_raw_parts(
            /* 指向 _time_val 的字节指针 */
            ptr as *const TimeVal as *const u8,
            /* TimeVal 的字节大小 */
            core::mem::size_of::<TimeVal>(),
        )
    };
    trace!(
        "TimeVal: {:?}, Buffers length: {:?}",
        _time_val,
        buffers.len()
    );

    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&src[offset..offset + len]);
        // 更新 offset
        offset += len;
    }
    0
}

// Range validation for anonymous mappings and unmapping.
// Source: https://learningos.cn/rCore-Tutorial-Guide/chapter4/7exercise.html
// Check actual endpoints before VirtAddr truncates them to 39 bits. The exclusive
// end may be the boundary of a canonical half, so validate end - 1 instead.
fn user_page_range(start: usize, len: usize) -> Option<(VirtPageNum, VirtPageNum)> {
    if len == 0 || start % PAGE_SIZE != 0 {
        return None;
    }
    let end = start.checked_add(len)?;
    let last = end - 1;
    let start_va = VirtAddr::from(start);
    let last_va = VirtAddr::from(last);
    if usize::from(start_va) != start
        || usize::from(last_va) != last
        || ((start ^ last) & (1usize << 38)) != 0
    {
        return None;
    }
    Some((start_va.floor(), VirtPageNum(last_va.floor().0 + 1)))
}

/// Allocate anonymous user pages with the requested read/write/execute permissions.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    if start % PAGE_SIZE != 0 || (prot & 0b111) == 0 || (prot & !0b111) != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let (start_vpn, end_vpn) = match user_page_range(start, len) {
        Some(range) => range,
        None => return -1,
    };
    let mut permission = MapPermission::U;
    if prot & 1 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 2 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 4 != 0 {
        permission |= MapPermission::X;
    }
    if map_current_pages(start_vpn, end_vpn, permission) {
        0
    } else {
        -1
    }
}

/// Remove all user mappings covering a page-aligned byte range.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if len == 0 {
        return 0;
    }
    let (start_vpn, end_vpn) = match user_page_range(start, len) {
        Some(range) => range,
        None => return -1,
    };
    if unmap_current_pages(start_vpn, end_vpn) {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// Create a child directly from its ELF without copying the parent's address space.
pub fn sys_spawn(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let data = inode.read_all();
        let parent = current_task().unwrap();
        let new_task = Arc::new(TaskControlBlock::new(data.as_slice()));
        let new_pid = new_task.getpid();
        new_task.inner_exclusive_access().parent = Some(Arc::downgrade(&parent));
        parent
            .inner_exclusive_access()
            .children
            .push(new_task.clone());
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

/// Set the current process's scheduling priority; values below two are invalid.
pub fn sys_set_priority(prio: isize) -> isize {
    if prio < 2 {
        return -1;
    }
    let task = current_task().unwrap();
    task.inner_exclusive_access().priority = prio as usize;
    prio
}

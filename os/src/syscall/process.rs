//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, get_syscall_count, suspend_current_and_run_next},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// AI assistance: request handling and pointer access discussed/reviewed with ChatGPT/Codex.
// Reference: https://learningos.cn/rCore-Camp-Guide-2025S/chapter3/5exercise.html
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    if _trace_request==2 {
        return get_syscall_count(_id) as isize
    }

    if _trace_request == 0 {
        let _ptr = _id as *const u8;
        let byte = unsafe{
            *_ptr
        };
        return byte as isize
    }

    if _trace_request == 1{
        let ptr = _id as *mut u8;
        unsafe{
            *ptr = _data as u8;
        }
        return 0

    }
    -1

}

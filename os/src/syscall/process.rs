//! Process management syscalls
use crate::config::{MAX_SYSCALL_NUM,PAGE_SIZE};
use crate::{mm::VirtAddr,task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next}, timer::get_time_us};
use crate::mm::translated_byte_buffer;
use crate::task::{current_user_token, get_syscall_count, map_current_pages, unmap_current_pages};
use crate::mm::{MapPermission, PageTable, PTEFlags, VirtPageNum};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// Write the timer value to user memory, including when TimeVal spans two pages.
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let _t = get_time_us();
    let _time_val = TimeVal{sec: _t/1_000_000,usec:_t%1_000_000};
    let buffers = translated_byte_buffer(current_user_token(), _ts as *const u8, core::mem::size_of::<TimeVal>());
    let ptr = &_time_val;
    let src: &[u8] = unsafe {
    core::slice::from_raw_parts(    
        /* 指向 _time_val 的字节指针 */
        ptr as *const TimeVal as *const u8,
        /* TimeVal 的字节大小 */
        core::mem::size_of::<TimeVal>()
        
    )};
        trace!("TimeVal: {:?}, Buffers length: {:?}", _time_val, buffers.len());  

    let mut offset = 0;
    for buffer in buffers {
        let len = buffer.len();
        buffer.copy_from_slice(&src[offset..offset+len]);
        // 更新 offset  
        offset += len;
        }
    0
}

/// Read or write a user byte, or query the current task's syscall counts.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    match _trace_request {
        0 =>{
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            let va = VirtAddr::from(_id);
            if usize::from(va) != _id {
               return -1;
            }
            let vpn = va.floor();
            let pte_result = page_table.translate(vpn);
            match pte_result{
                Some(pte) => {
                    if !pte.is_valid() || (pte.flags() & PTEFlags::U) == PTEFlags::empty() || !pte.readable(){
                        return -1;
                    }
                    let ppn = pte.ppn();
                    let bytes = ppn.get_bytes_array();
                    let offset = va.page_offset();
                    return bytes[offset] as isize;
                },
                None => return -1
            }

        }
        1 => {
            let token = current_user_token();
            let page_talble = PageTable::from_token(token);
            let va = VirtAddr::from(_id);
            if usize::from(va) != _id {
                return -1;
            }
            let vpn = va.floor();
            let pte_result = page_talble.translate(vpn);
            match pte_result{
                Some(pte) => {
                    if !pte.is_valid() || (pte.flags()& PTEFlags::U)==PTEFlags::empty() || !pte.writable(){
                        return -1;
                    }
                    let ppn = pte.ppn();
                    let bytes = ppn.get_bytes_array();
                    let offset = va.page_offset();
                    bytes[offset] = _data as u8;
                    return 0;
                }
                None => return -1
            }
        }

        2 => {
            if _id >= MAX_SYSCALL_NUM {
                return -1;
            }
            get_syscall_count(_id) as isize
        }
        _ => {return -1}
    }
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
    if map_current_pages(start_vpn, end_vpn, permission) { 0 } else { -1 }
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
    if unmap_current_pages(start_vpn, end_vpn) { 0 } else { -1 }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

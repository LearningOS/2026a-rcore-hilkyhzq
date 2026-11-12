//! Process management syscalls
use crate::task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next};

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

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    let us = crate::timer::get_time_us();
    let time = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let Some(buffers) = crate::mm::checked_user_buffer(
        crate::task::current_user_token(),
        ts as usize,
        core::mem::size_of::<TimeVal>(),
        true,
    ) else {
        return -1;
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &time as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    let mut offset = 0;
    for buffer in buffers {
        let count = buffer.len();
        buffer.copy_from_slice(&bytes[offset..offset + count]);
        offset += count;
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        0 | 1 => {
            let Some(mut buffers) = crate::mm::checked_user_buffer(
                crate::task::current_user_token(),
                id,
                1,
                trace_request == 1,
            ) else {
                return -1;
            };
            let byte = &mut buffers[0][0] as *mut u8;
            if trace_request == 0 {
                unsafe { byte.read_volatile() as isize }
            } else {
                unsafe {
                    byte.write_volatile(data as u8);
                }
                0
            }
        }
        2 => crate::task::current_syscall_count(id) as isize,
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    crate::task::with_current_memory_set(|memory| memory.map_user_range(start, len, port))
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    crate::task::with_current_memory_set(|memory| memory.unmap_user_range(start, len))
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

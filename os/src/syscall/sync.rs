use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

const DEADLOCK: isize = -0xDEAD;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = process_inner.mutex_list.len();
    let mutex: Arc<dyn Mutex> = if !blocking {
        Arc::new(MutexSpin::new(id))
    } else {
        Arc::new(MutexBlocking::new(id))
    };
    process_inner.mutex_list.push(Some(mutex));
    process_inner.mutex_resources.add_resource(1);
    id as isize
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = match process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
    {
        Some(mutex) => Arc::clone(mutex),
        None => return -1,
    };
    drop(process_inner);
    drop(process);
    if mutex.lock() {
        0
    } else {
        DEADLOCK
    }
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = match process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
    {
        Some(mutex) => Arc::clone(mutex),
        None => return -1,
    };
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    if res_count > isize::MAX as usize {
        return -1;
    }
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = process_inner.semaphore_list.len();
    process_inner
        .semaphore_list
        .push(Some(Arc::new(Semaphore::new(res_count, id))));
    process_inner.semaphore_resources.add_resource(res_count);
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = match process_inner
        .semaphore_list
        .get(sem_id)
        .and_then(|item| item.as_ref())
    {
        Some(sem) => Arc::clone(sem),
        None => return -1,
    };
    drop(process_inner);
    drop(process);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = match process_inner
        .semaphore_list
        .get(sem_id)
        .and_then(|item| item.as_ref())
    {
        Some(sem) => Arc::clone(sem),
        None => return -1,
    };
    drop(process_inner);
    drop(process);
    if sem.down() {
        0
    } else {
        DEADLOCK
    }
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = match process_inner
        .condvar_list
        .get(condvar_id)
        .and_then(|item| item.as_ref())
    {
        Some(condvar) => Arc::clone(condvar),
        None => return -1,
    };
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = match process_inner
        .condvar_list
        .get(condvar_id)
        .and_then(|item| item.as_ref())
    {
        Some(condvar) => Arc::clone(condvar),
        None => return -1,
    };
    let mutex = match process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
    {
        Some(mutex) => Arc::clone(mutex),
        None => return -1,
    };
    drop(process_inner);
    drop(process);
    if condvar.wait(mutex) {
        0
    } else {
        DEADLOCK
    }
}
/// enable deadlock detection syscall
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled > 1 {
        return -1;
    }
    current_process()
        .inner_exclusive_access()
        .deadlock_detect_enabled = enabled == 1;
    0
}

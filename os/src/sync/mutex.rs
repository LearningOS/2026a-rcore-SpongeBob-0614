//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_process, current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self) -> bool;
    /// Unlock the mutex
    fn unlock(&self);
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    id: usize,
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new(id: usize) -> Self {
        Self {
            id,
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) -> bool {
        trace!("kernel: MutexSpin::lock");
        let tid = current_task().unwrap().gettid();
        {
            let process = current_process();
            let mut inner = process.inner_exclusive_access();
            let detect = inner.deadlock_detect_enabled;
            if !inner.mutex_resources.request(tid, self.id, detect) {
                return false;
            }
        }
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                current_process()
                    .inner_exclusive_access()
                    .mutex_resources
                    .acquire(tid, self.id);
                return true;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        current_process()
            .inner_exclusive_access()
            .mutex_resources
            .release(current_task().unwrap().gettid(), self.id);
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    id: usize,
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new(id: usize) -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            id,
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) -> bool {
        trace!("kernel: MutexBlocking::lock");
        let tid = current_task().unwrap().gettid();
        {
            let process = current_process();
            let mut inner = process.inner_exclusive_access();
            let detect = inner.deadlock_detect_enabled;
            if !inner.mutex_resources.request(tid, self.id, detect) {
                return false;
            }
        }
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            current_process()
                .inner_exclusive_access()
                .mutex_resources
                .acquire(tid, self.id);
        }
        true
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner
            .mutex_resources
            .release(current_task().unwrap().gettid(), self.id);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            process_inner
                .mutex_resources
                .acquire(waking_task.gettid(), self.id);
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
    }
}

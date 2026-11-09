//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{
    block_current_and_run_next, current_process, current_task, wakeup_task, TaskControlBlock,
};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    id: usize,
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize, id: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            id,
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner
            .semaphore_resources
            .release(current_task().unwrap().gettid(), self.id);
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                process_inner
                    .semaphore_resources
                    .acquire(task.gettid(), self.id);
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) -> bool {
        trace!("kernel: Semaphore::down");
        let tid = current_task().unwrap().gettid();
        {
            let process = current_process();
            let mut process_inner = process.inner_exclusive_access();
            let detect = process_inner.deadlock_detect_enabled;
            if !process_inner
                .semaphore_resources
                .request(tid, self.id, detect)
            {
                return false;
            }
        }
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        } else {
            current_process()
                .inner_exclusive_access()
                .semaphore_resources
                .acquire(tid, self.id);
        }
        true
    }
}

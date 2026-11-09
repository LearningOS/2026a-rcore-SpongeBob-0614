//!Implementation of [`TaskManager`]
// stride 调度依据第五章练习；选择、步长及溢出归一化由 AI 辅助实现与验证。
// https://learningos.cn/rCore-Tutorial-Guide/chapter5/4exercise.html
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

// Larger than any accepted priority, so even isize::MAX makes progress.
const BIG_STRIDE: u128 = 1u128 << 64;

///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// Stride scheduler with FIFO ordering for equal strides.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Choose the smallest stride and charge it for one scheduling turn.
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let (index, min_stride) = self
            .ready_queue
            .iter()
            .enumerate()
            .map(|(index, task)| (index, task.inner_exclusive_access().stride))
            .min_by_key(|&(_, stride)| stride)?;
        let priority = self.ready_queue[index].inner_exclusive_access().priority;
        let pass = (BIG_STRIDE / priority as u128).max(1);

        if min_stride.checked_add(pass).is_none() {
            // Every runnable process is in this queue when the processor fetches.
            // Subtracting their common minimum preserves ordering and differences,
            // and gives the selected process room for its next stride increment.
            for task in &self.ready_queue {
                task.inner_exclusive_access().stride -= min_stride;
            }
        }

        let task = self.ready_queue.remove(index).unwrap();
        task.inner_exclusive_access().stride += pass;
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

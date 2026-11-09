//! Resource accounting and deadlock detection for process-local synchronization.

use alloc::vec;
use alloc::vec::Vec;

pub(crate) struct ResourceTracker {
    available: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl ResourceTracker {
    pub(crate) fn new() -> Self {
        Self {
            available: Vec::new(),
            allocation: Vec::new(),
            need: Vec::new(),
        }
    }

    pub(crate) fn add_resource(&mut self, count: usize) {
        self.available.push(count);
        for row in &mut self.allocation {
            row.push(0);
        }
        for row in &mut self.need {
            row.push(0);
        }
    }

    fn ensure_thread(&mut self, tid: usize) {
        while self.allocation.len() <= tid {
            self.allocation.push(vec![0; self.available.len()]);
            self.need.push(vec![0; self.available.len()]);
        }
    }

    pub(crate) fn request(&mut self, tid: usize, resource: usize, detect: bool) -> bool {
        self.ensure_thread(tid);
        self.need[tid][resource] += 1;
        if detect && !self.is_safe() {
            self.need[tid][resource] -= 1;
            return false;
        }
        true
    }

    pub(crate) fn acquire(&mut self, tid: usize, resource: usize) {
        self.ensure_thread(tid);
        self.need[tid][resource] -= 1;
        self.available[resource] -= 1;
        self.allocation[tid][resource] += 1;
    }

    pub(crate) fn release(&mut self, tid: usize, resource: usize) {
        self.ensure_thread(tid);
        if self.allocation[tid][resource] > 0 {
            self.allocation[tid][resource] -= 1;
        }
        self.available[resource] += 1;
    }

    pub(crate) fn retire_thread(&mut self, tid: usize) {
        self.ensure_thread(tid);
        self.allocation[tid].fill(0);
        self.need[tid].fill(0);
    }

    pub(crate) fn reset_thread(&mut self, tid: usize) {
        self.retire_thread(tid);
    }

    fn is_safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finished = vec![false; self.allocation.len()];
        loop {
            let mut progress = false;
            for tid in 0..self.allocation.len() {
                if finished[tid]
                    || self.need[tid]
                        .iter()
                        .zip(&work)
                        .any(|(need, available)| need > available)
                {
                    continue;
                }
                for (available, allocated) in work.iter_mut().zip(&self.allocation[tid]) {
                    *available += allocated;
                }
                finished[tid] = true;
                progress = true;
            }
            if !progress {
                return finished.iter().all(|finished| *finished);
            }
        }
    }
}

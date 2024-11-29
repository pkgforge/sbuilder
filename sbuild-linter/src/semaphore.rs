use std::sync::{Condvar, Mutex};

pub struct Semaphore {
    permits: Mutex<usize>,
    condvar: Condvar,
}

impl Semaphore {
    pub fn new(count: usize) -> Self {
        Semaphore {
            permits: Mutex::new(count),
            condvar: Condvar::new(),
        }
    }

    pub fn acquire(&self) {
        let mut permits = self.permits.lock().unwrap();
        while *permits == 0 {
            permits = self.condvar.wait(permits).unwrap();
        }
        *permits -= 1;
    }

    pub fn release(&self) {
        let mut permits = self.permits.lock().unwrap();
        *permits += 1;
        self.condvar.notify_one();
    }
}

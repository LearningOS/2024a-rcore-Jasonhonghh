//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};
use crate::task::current_process;
/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self,sem_id:usize) {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        if inner.count <= 0 {
            //获取需要唤醒的任务
            if let Some(task) = inner.wait_queue.pop_front() {
                //注意唤醒操作需要获取task的所有权，因此我们不能提前转移所有权
                //获取当前process和唤醒的task的inner
                //当前task是Arc<TaskControlBlock>类型
                println!("wakeuppppppppppppppppppp");
                let process = current_process();
                let mut next_inner = task.inner_exclusive_access();
                let mut process_inner = process.inner_exclusive_access();
                let tid = next_inner.res.as_ref().unwrap().tid;
                let deadlocktest = process_inner.deadlocktest;
                if deadlocktest&&tid!=0&&sem_id!=0 {
                    //如果开启了死锁检测，且不是第一个线程，且不是第一个信号量
                    process_inner.work[sem_id]-=1;
                    next_inner.allocate[sem_id]+=1;
                    next_inner.need[sem_id]-=1;
                }
                println!("wakeuppppppppppppppppppp");
                drop(process_inner);
                //把原来的task的内容复制，新建一个task
                drop(next_inner);
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        }
    }
}

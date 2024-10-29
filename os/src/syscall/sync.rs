use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec::Vec;
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
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
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
    let deadlocktest = process_inner.deadlocktest;
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    if deadlocktest {
        if mutex.is_locked() {
            return -0xdead;
        }
    }
    drop(process_inner);
    drop(process);
    mutex.lock();
    0
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
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
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
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    process_inner.work.push(res_count);//todo添加信号量的资源数
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
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
    //找到task和process
    let task = current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    //找到task和process
    let tid = task_inner.res.as_ref().unwrap().tid;
    let deadlocktest = process_inner.deadlocktest;
    if deadlocktest&&tid!=0&&sem_id!=0 {
        //如果开启了死锁检测，且不是第一个线程，且不是第一个信号量
        process_inner.work[sem_id] += 1;
        task_inner.allocate[sem_id]-=1;
    }
    drop(task_inner);
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up(sem_id);
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
    //找到task和process
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    //找到task和process
    let resct = process_inner.work[sem_id];//获取信号量的资源数
    let tid = task_inner.res.as_ref().unwrap().tid;
    let deadlocktest = process_inner.deadlocktest;
    drop(task_inner);
    if deadlocktest&&tid!=0&&sem_id!=0{
        //如果这个资源还有的话，检查分配给他的情况
        if resct>0{
            //如果开启了死锁检测，且不是第一个线程，且不是第一个信号量,此时需要进行死锁检测
            //克隆一个work数组，对应资源减1
            let mut workvec = process_inner.work.clone();
            workvec[sem_id] -= 1;
            //设置finish数组
            let mut finish = Vec::new();
            //所有线程默认为false
            finish.push(false);
            //检查所有的线程的need数组
            for i in 1..process_inner.thread_count(){
                let thread = process_inner.get_task(i);
                let thead_inner = thread.inner_exclusive_access();
                let need = thead_inner.need.clone();
                drop(thead_inner);
                let mut finish_flag = true;//假设所有任务都可以完成
                println!("test tid:{} sem_id:{}",tid,sem_id);
                println!("workvec:{:?},need:{:?}",workvec,need);
                for j in 1..workvec.len(){
                    if need[j]>workvec[j]{
                        //如果有线程的need数组大于work数组，说明这个任务不能完成
                        finish_flag = false;
                    }
                }
                //检查完毕这个线程的need数组，将结果加入finish数组
                finish.push(finish_flag);
            }
            //检查finish数组，如果有线程可以完成，那么就不会死锁
            if finish.contains(&true) {
                //存在true的话，说明有线程可以完成
                let task = current_task().unwrap();
                let mut task_inner = task.inner_exclusive_access();
                process_inner.work[sem_id] -= 1;
                task_inner.allocate[sem_id] += 1;
                let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
                drop(process_inner);
                drop(task_inner);
                sem.down();
                println!("no deadlock");
                return 0;
            }
            //如果没有线程可以完成，那么就会死锁
            println!("deadlock");
            return -0xdead;
        }else {//如果这个资源没有了，需要考虑need的情况。
            //如果开启了死锁检测，且不是第一个线程，且不是第一个信号量,此时需要进行死锁检测
            //克隆一个work数组，对应资源减1
            let workvec = process_inner.work.clone();
            //设置finish数组
            let mut finish = Vec::new();
            //所有线程默认为false
            finish.push(false);
            //检查所有的线程的need数组
            for i in 1..process_inner.thread_count(){
                let thread = process_inner.get_task(i);
                let thead_inner = thread.inner_exclusive_access();
                let mut need = thead_inner.need.clone();
                if need.len()==4 {
                    need[sem_id] += 1
                }else {
                    if tid == i{
                        need[sem_id]+=1;
                    }
                }
                drop(thead_inner);
                let mut finish_flag = true;//假设所有任务都可以完成
                println!("test tid:{} sem_id:{}",tid,sem_id);
                println!("workvec:{:?},need:{:?}",workvec,need);
                for j in 1..workvec.len(){
                    if need[j]>workvec[j]{
                        //如果有线程的need数组大于work数组，说明这个任务不能完成
                        finish_flag = false;
                    }
                }
                //检查完毕这个线程的need数组，将结果加入finish数组
                finish.push(finish_flag);
            }
            //检查finish数组，如果有线程可以完成，那么就不会死锁
            if finish.contains(&true) {
                //存在true的话，说明有线程可以完成
                let task = current_task().unwrap();
                let mut task_inner = task.inner_exclusive_access();
                process_inner.work[sem_id] -= 1;
                task_inner.allocate[sem_id] += 1;
                let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
                drop(process_inner);
                drop(task_inner);
                sem.down();
                println!("no deadlock");
                return 0;
            }
            //如果没有线程可以完成，那么就会死锁
            println!("deadlock");
            return -0xdead;
        }
    }
    //如果没有开启死锁检测，或者是第一个线程，或者是第一个信号量，那么就直接down
    //如果是没有资源，那么就会阻塞，我们直接在这里给need+1,
    if deadlocktest&&tid!=0&&sem_id!=0&&resct==0{
        let task = current_task().unwrap();
        let mut task_inner = task.inner_exclusive_access();
        task_inner.need[sem_id] += 1;
        drop(task_inner);
    }

    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.down();
    0
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
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
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
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(_enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect NOT IMPLEMENTED");
    if _enabled>1{
        return -1;
    }
    if _enabled==1 {
        let process = current_process();
        let mut process_inner = process.inner_exclusive_access();
        process_inner.deadlocktest = true;
    }
    0
}

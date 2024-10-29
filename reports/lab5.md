# sys_enable_deadlock_detect

```
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
```

给PCB中添加一个新的字段deadlocktest。true表示启用死锁检测。

```
pub struct ProcessControlBlockInner {
    ///deadlocktest
    pub deadlocktest: bool,
}
```

PCB的new，增加初始化字段。fork方法类似。

```
pub fn new(elf_data: &[u8]) -> Arc<Self> {
        trace!("kernel: ProcessControlBlock::new");
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, ustack_base, entry_point) = MemorySet::from_elf(elf_data);
        // allocate a pid
        let pid_handle = pid_alloc();
        let process = Arc::new(Self {
            pid: pid_handle,
            inner: unsafe {
                UPSafeCell::new(ProcessControlBlockInner {
                    deadlocktest: false,
                    is_zombie: false,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(Arc::new(Stdin)),
                        // 1 -> stdout
                        Some(Arc::new(Stdout)),
                        // 2 -> stderr
                        Some(Arc::new(Stdout)),
                    ],
                    signals: SignalFlags::empty(),
                    tasks: Vec::new(),
                    task_res_allocator: RecycleAllocator::new(),
                    mutex_list: Vec::new(),
                    semaphore_list: Vec::new(),
                    condvar_list: Vec::new(),
                })
            },
        });
```

到此完成了这个系统调用的编写。接下来在两种机制中检测死锁时，需要先检测这个PCB中的这个标志位。

# Mutex机制中死锁的检测

下面只是为了通过测例，检测的问题，不能归为死锁。

给Mutex trait添加一个is_locked方法，让MutexBlocking和MutexSpin实现它。

```
pub trait Mutex: Sync + Send {
    /// is_locked
    fn is_locked(&self) -> bool;
}
impl Mutex for MutexBlocking {
    /// is_locked
    fn is_locked(&self) -> bool {
        let mutex_inner = self.inner.exclusive_access();
        mutex_inner.locked
    }
}
impl Mutex for MutexSpin {
    /// is_locked
    fn is_locked(&self) -> bool {
        let locked = self.locked.exclusive_access();
        *locked
    }
}
```

修改sys_mutex_lock系统调用

```
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
```

至此只有和信号量相关的两个系统调用没有实现。

# 信号量机制死锁检测

semaphore_list中索引为0的元素代表线程信号量，不用管，一个进程中的所有进程共享从1-n位置的信号量。我们需要考虑tid=1~m个线程的的死锁问题，不用考虑tid=0的主线程。

## 新建几个数据结构：

1. PCB新建work数组，和semaphore_list索引的资源对应，但work表示当前可用的资源数目。所以work应该使用usize表示。
2. TCB新建allocate数组，和semaphore_list索引的资源对应，表示当前线程已经分配到的资源。
3. TCB新建need数组，和semaphore_list索引的资源对应，表示当前线程需要但是还未分配的资源，进程处于该资源的等待队列中。

```
pub struct ProcessControlBlockInner {
    /// allocate
    pub work: Vec<usize>,
}
pub struct TaskControlBlockInner {
    /// allocate
    pub allocate:Vec<usize>,
    /// need
    pub need: Vec<usize>,
}
```

在PCB的new和fork方法中，work数组的初始值为空的数组。测试程序的主线程会调用sys_semaphore_create来新增资源信号量，此时我们进行work数组的初始化。

```
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
```

接下来我们初始化TCB的两个数组，allocate和need，首先读取work的长度，再新建数组，在赋值到结构体中。

需要在task的task.rs中引入Vec。

```
use core::alloc::vec::Vec;
impl TaskControlBlock {
    /// Create a new task
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        let res = TaskUserRes::new(Arc::clone(&process), ustack_base, alloc_user_res);
        let trap_cx_ppn = res.trap_cx_ppn();
        let kstack = kstack_alloc();
        let kstack_top = kstack.get_top();
        let worklen = process.inner_exclusive_access().work.len();
        let mut allocate = Vec::new();
        let mut need = Vec::new();
        for _i in 0..worklen {
            allocate.push(0usize);
            need.push(0usize);
        }
        Self {
            process: Arc::downgrade(&process),
            kstack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    allocate: allocate,
                    need: need,
                    res: Some(res),
                    trap_cx_ppn,
                    task_cx: TaskContext::goto_trap_return(kstack_top),
                    task_status: TaskStatus::Ready,
                    exit_code: None,
                })
            },
        }
    }
}
```

此时我们已经完成了三个数组的创建和初始化工作。

## 实现数组更新机制

1. work-allocate修改。因为这两个都是对应实际资源的剩余情况和占用情况，所以现在work和allocate应该成对修改。理论上三种情况需要修改：（就是实际上线程获取和释放资源的时候）
    1. 线程A进行up操作。
    2. 线程A进行退出操作。（由于测试例子中都是PV操作成对出现，所以这种情况不考虑）
    3. 线程A进行down操作时直接拿到了资源。
    4. 容易忽视的情况是，别的线程执行up操作，唤醒线程A时。
2. 对于need修改。
    1. 线程A进入等待队列。即down发现资源不够时，需要增加。
    2. 线程A出等待队列，有其他线程执行up操作唤醒线程A，需要减少。

我们在up和down的系统调用中实现一部分修改，然后在信号量的up和down中实现一部分修改。

------

为了避免影响其他测试我们下面所有的操作都判断deallocktest、tid、sem_id

### sys_semaphore_up系统调用 和up方法

线程主动释放这个资源时，我们肯定需要把allocate-1,work+1。但是线程主动释放资源时，可能唤起其他的等待的线程，我们需要把这些线程allocate+1,work-1，然后need-1。

```
//sys_semaphore_up
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
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(task_inner);
    drop(process_inner);
    sem.up(sem_id);
    0
}
```

sem.up如果唤起其他的进程，我们需要进行把这些线程allocate+1,work-1，然后need-1。因此修改这个Semaphore的up方法，首先加入一个参数。然后在需要唤醒之前执行一些操作。唤醒之前，先检查deadlocktest&&tid!=0&&sem_id!=0。

```
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
                //获取一个Arc的clone
                let next_task = Arc::clone(&task);
                let process = current_process();
                let next_inner = next_task.inner.exclusive_access();
                let process_inner = process.inner_exclusive_access();
                let tid = next_inner.res.as_ref().unwrap().tid;
                let deadlocktest = process_inner.deadlocktest;
                if deadlocktest&&tid!=0&&sem_id!=0 {
                    //如果开启了死锁检测，且不是第一个线程，且不是第一个信号量
                    process_inner.work[sem_id]-=1;
                    next_inner.allocate[sem_id]+=1;
                    next_inner.need[sem_id]-=1;
                }
                wakeup_task(task);
            }
        }
    }
```



### sys_semaphore_down系统调用和down方法

线程请求这个资源时有两种情况

1. work>0时，我们需要检查是否出现死锁。
    1. 如果死锁直接返回-0xdead。
    2. 没有死锁的话，allocate+1,work-1。
2. work=0时，我们需要把当前元素加入到等待队列，此时不用修改work和allocate。因为压根没有分配，但是need需要加1。

**这些逻辑我们都可以在sys_semaphore_down系统调用中实现。**



## 实现检测算法

**所谓死锁检测，就是当我们此时可以为一个线程分配一个资源的时候，考虑：如果我把这个资源分配给当前线程，剩下的资源够不够某一个进程当前的need，如果所有的进程需要的资源都大于此时的剩余资源，那么所有的进程都无法完成。**

因此我们在down的系统调用中，假设我已经分配了当前请求的资源给当前的线程，（其实就是work[i]-1），之后我遍历除了主线程的其他所有线程的need数组，如果work中剩下的所有资源，遍历的线程都不够用，说明出现了死锁。

最后的sys_semaphore_down系统调用为

```
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
                need[sem_id]+=1;
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
```



ch5需要通过前面几章的测例，但是ch5的源码把包括任务控制块在内的一些数据结构做了修改，再加上我前面有一些实现的规范的地方（例如ch4中直接把一些结构体字段写成pub），所以这章我会首先以规范的方式重写sys_get_time、sys_task_info、sys_mmap、sys_munmap的内容，便于后面ch6、ch8的解答，最后再实现ch5需要实现的spawn系统调用和stride调度算法。

测试的时候需要注意，ci-user中`make test CHPTER=5`，执行的是ch5_usertests.rs，不是ch5b_usertests.rs。

# sys_get_time

这个系统调用和第四章实现的方法是一样的。

```syscall/process.rs
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let buffers =
        translated_byte_buffer(current_user_token(), _ts as *const u8, size_of::<TimeVal>());
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let mut time_val_ptr = &time_val as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            time_val_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            time_val_ptr = time_val_ptr.add(buffer.len());
        }
    }
    0
}
```

上面的代码使用了mm模块的translated_byte_buffer、task模块的current_user_token函数、timer模块的get_time_us函数，核心库mem模块的size_of函数。而且这个系统调用是进程无关的，因此只需要**导入这三个函数**即可。

# sys_task_info

原本的TaskManager在这章被拆分了，因此需要考虑一些新的问题。

Q1：如果一个进程创建了一个子进程，那这个子进程的系统调用次数和系统调用距离任务开始时间怎么算呢？

A1：教程要求通过前面几个章节的测例，这个问题需要通过查看测试例子来决定怎么写。

Q2：ch4的方法涉及到在TaskControlBlock中增加新的字段，ch5应该怎么增加和初始化？

A2：需要好好理解TaskControlBlock和TaskManager拆分后的功能。

首先在ci-user下运行`make test CHPTER=5`，看下运行了哪些测试。ci-user实际应该只是执行了ch5b_usertest.rs。这个里面没有涉及获取sys_task_info的测试，并且所有的测试都是通过spawn调用为子进程，此时我们还没有实现spawn，所以下面所有的测试实际都还没有运行，所以我们先跳过sys_task_info的实现，先实现spawn系统调用。

```
Usertests: Running ch2b_hello_world
Usertests: Running ch2b_power_3
Usertests: Running ch2b_power_5
Usertests: Running ch2b_power_7
Usertests: Running ch3b_yield0
Usertests: Running ch3b_yield1
Usertests: Running ch3b_yield2
Usertests: Running ch3_sleep
Usertests: Running ch3_sleep1
Usertests: Running ch4_mmap0
Usertests: Running ch4_mmap1
Usertests: Running ch4_mmap2
Usertests: Running ch4_mmap3
Usertests: Running ch4_unmap
Usertests: Running ch4_unmap2
Usertests: Running ch5_spawn0
Usertests: Running ch5_spawn1
Usertests: Running ch5_setprio
```

# sys_spawn

先获取当前任务，再调用当前任务的spawn方法，创建并执行一个新进程。

```sycall/process.rs
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        let new_task = task.spawn(data);//spawn is fork+exec,return newtask
        let new_pid = new_task.pid.0;
        let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
        // we do not have to move to next instruction since we have done it before
        // for child process, fork returns 0
        trap_cx.x[10] = 0;
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}
```

这里的sys_spawn和sys_exec很类似，不过返回值不同。sys_spawn调用task模块的spawn函数，spawn的实现逻辑是调用task模块的fork函数，然后调用exec函数，最后返回一个new_task。我们将spawn定义为TaskControlBlock的内联函数，所以不用在syscall/process.rs中直接引用。这里我没有重新实现，只是调用fork+exec，不是很推荐这样做，因为太简单了。

```
impl TaskControlBlock{
    pub fn spawn(self: &Arc<Self>, elf_data: &[u8]) -> Arc<Self> {
        let new_task = self.fork();
        new_task.exec(elf_data);
        new_task
    }
}
```

接下来再在ci-user下运行`make test CHPTER=5`，可以看到spawn成功将所有的进程创建执行了，没有通过的测试和对ch4mmap、munmap函数以及sys_set_priority的测试有关。接下来我们先实现第4章的mmap和munmap，上一章对munmap实现不够规范，这里我们进行规范的实现。

# sys_mmap

在ch4中我们定义并调用了TASK_MANAGER的task_mmap_area方法，来实现对一片虚拟内存的映射，但是ch5中，把TaskManger的功能解耦到manager和processor。我们需要做一些小的改动，把大部分的代码逻辑就在sys_mmap实现。首先类比ch4的思路，补充sys_mmap，先检查参数合法性，再检查对应区间是否映射，最后执行映射。

```
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _start % 4096 != 0 { return -1; } //start没有对齐到页
    if _prot & !0x7 != 0 { return -1; } //prot除最后3位外其余的位不为0
    if _prot & 0x7 == 0 { return -1; } //prot的最后三位为0
    // 物理空间不足，暂时不处理
    let mut permission = MapPermission::from_bits((_prot as u8) << 1).unwrap();
    permission.set(MapPermission::U, true); //把prot转换为permission
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    //虚拟页号
    let start_vpn = _start / 4096;
    let end_vpn = (_start + _len + 4095) / 4096;//最后一页的后一页的页号
    for vpn in start_vpn..end_vpn {
        let c_vpn:VirtPageNum = vpn.into();
        if page_table.find_pte(c_vpn).is_some(){//如果已经映射了
            return -1;
        }
    }
    let task = current_task().unwrap();
    let memory_set = &mut task.inner_exclusive_access().memory_set;
    //传入虚拟内存地址，之后调用的函数会自动转换为页号，向下向上取整
    let start_va = VirtAddr::from(_start);
    let end_va = VirtAddr::from(_start + _len);//注意这个地方是减一
    memory_set.insert_framed_area(start_va,end_va, permission);
    0
}
```

注意

1. 引入PageTable、VirtPageNum、VirtAddr、MapPermission。并把对应的mod改为pub use PageTable的格式。
2. 把pagetable的find_pte方法标记为pub。
3. 给pagetable的bitflags!添加文档注释。

执行完上面这些步骤就只有两个测试没有通过了，unmap和set_priority。加油

# sys_munmap

想了一下，还是按照ch4的方法比较简单，

```
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let memory_set = &mut task.inner_exclusive_access().memory_set;
    let mut area_id = 0;
    let page_table =&mut memory_set.page_table;
    for area in memory_set.areas.iter_mut(){
        // println!("vpn_range start:{:?} end:{:?}",area.vpn_range.get_start().0,area.vpn_range.get_end().0);
        // println!("start:{:?} end:{:?}",_start/4096,(_start+_len)/4096);
        if area.vpn_range.get_start().0*4096 == _start && area.vpn_range.get_end().0*4096 == _start+_len{
            // println!("find area");
            area.unmap(page_table);
            // println!("find area");
            memory_set.areas.remove(area_id);
            // println!("find area");
            return 0;
        }
        area_id += 1;
    }
    -1
}
```

注意：把memory的两个字段设置成pub，并且给出文档注释。area的vpn_range字段设置为pub，给出文档注释。还有一点，需要修改一个查询逻辑，这个坑我已经踩过了。

```
    pub fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if !pte.is_valid() {
                return None;
            }
            if i == 2 {
                result = Some(pte);
                break;
            }
            ppn = pte.ppn();
        }
        result
    }
```

现在只有set_priority没有通过，我们需要实现stride 调度算法。

# stride 调度算法

我们首先要在TaskControlBlock中增加新的字段，然后修改fetch时的算法，从当前 runnable 态的进程中选择 stride 最小的进程调度。对于获得调度的进程 P，将对应的 stride 加上其对应的步长 pass。

修改TaskControlBlockInner内部的成员变量，新增priority和stride。

```
pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_cx_ppn: PhysPageNum,

    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    pub base_size: usize,

    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    pub parent: Option<Weak<TaskControlBlock>>,

    /// A vector containing TCBs of all child processes of the current process
    pub children: Vec<Arc<TaskControlBlock>>,

    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// Priority
    pub task_priority: usize, 

    /// stride
    pub task_stride: usize,
}
```

指导书上指出：

- 进程初始 stride 设置为 0 即可。
- 进程初始优先级设置为 16。

因此我们需要在TaskControlBlockInner初始化的时候给出二者的初始值。具体位置在TCB的new方法。**还有fork方法也需要更新初始化**

```
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        // push a task context which goes to trap_return to the top of kernel stack
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: user_sp,
                    program_brk: user_sp,
                    task_priority: 16,
                    task_stride: 0,
                })
            },
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
```

在每次进程执行之后我们要增加其task_stride。在task模块的suspend_current_and_run_next函数中。这里为了避免麻烦我把BIG_STRIDE设置成了1000。

```
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // Change status to Ready
    task_inner.task_status = TaskStatus::Ready;
    task_inner.task_stride = task_inner.task_stride + 1000 / task_inner.task_priority;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_cx_ptr);
}
```

好了，现在每个进程的stride都会随着调用结束而更新了。现在我们需要补充一个系统调用，让测试程序能够自定义优先级。

为了补充sys_set_priority系统调用，我们获取当前进程的TaskControlBlockInner，然后把参数传给其task_priority字段。

```
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _prio <=1 {
        return -1;
    }
    let task = current_task().unwrap();
    let task_priority = &mut task.inner_exclusive_access().task_priority;
    *task_priority = _prio;
    0
}
```

好了，接下来我们需要修改fetch函数，每次拉一个stride最小的进程运行。原本的fetch函数是这样：

```
pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
```

这个ready_queue是一个双端队列，目前有两种方法来修改。

1. fetch时遍历双端队列，找到最小值，返回。
2. push的时候，注意按照stride大小来插入双端队列。

我选择第一种。构造一个新的双端队列，遍历原来的双端队列，找到最小值之后，把新的双端队列的数据重新插入原来的双端队列。

```
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let mut result_task = self.ready_queue.pop_front().as_ref()?.clone();
        let mut tmp_deque = VecDeque::new();
        let mut i = self.ready_queue.pop_front();
        while i.is_some() {
            let i_task = i.as_ref()?.clone();
            if i_task.inner_exclusive_access().task_stride >= result_task.inner_exclusive_access().task_stride {
                tmp_deque.push_back(i.clone());
            } else {
                tmp_deque.push_back(Some(result_task.clone()));
                result_task = i_task;
            }
            i = self.ready_queue.pop_front();
        }
        while!tmp_deque.is_empty() {
            self.ready_queue.push_back(tmp_deque.pop_front().unwrap().unwrap());
        }
        Some(result_task)
    }
```

现在可以通过所有的测试
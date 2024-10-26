# 第四章思路

注：省略了部分文档注释，一部分实际代码中不写会报错，这个添加比较容易。

## Q1 重写sys_get_time

原本的sys_get_time函数为

```
/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}
```

主要需要修改TimeVal

```
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}
```

现在的问题应该是引入分页机制之后，TimeVal的内容可能分布在两个页中。无法直接从*ts来写入所有数据。去年的参考答案给出了解决方法。

```
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
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

重点是通过translated_byte_buffer函数提供了一个访存的接口，现在的buffer看起来是一片连续的空间。我们只需要调用timer接口获取时间，然后复制到这个位置就可以了。但还有一些小问题还没有解决。

+ ~~current_user_token()用于获取当前程序的token，以便获取一级页表的物理页号。实际上在memory_set.rs中提供了一个内联函数接口，但是我不太清楚怎么获取这个memory_set的实例，所以我选择直接读取satp寄存器中的64位token。这个函数定义在page_table.rs中，记得写文档注释，不然会报错。~~

上面这段逻辑是错误的，系统调用时satp读取的内核的token，而我们需要获取的是调用的用户程序的token。现在仍然使用这个函数，但是修改函数的逻辑并且把这个函数放在task/mod.rs下调用TASK_MANAGER的一个私有方法。

```
/// hong:get the current user token  ！！！这是错误的做法
pub fn current_user_token() -> usize {
    let satp: usize;
    unsafe {
        core::arch::asm!("csrr {}, 0x180", out(reg) satp);
    }
    satp
}
```
正确做法是直接调用在task/mod.rs的current_user_token函数接口和mm/mod.rs中的translated_byte_buffer函数接口。



# Q2 重写sys_task_info

去年的答案没有，但结合ch3和Q1的答案，应该可以解答。

首先模仿Q1，利用translated_byte_buffer函数来获取访存接口。然后把对应的数据复制进来。

```
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info NOT IMPLEMENTED YET!");
    let buffers = translated_byte_buffer(current_user_token(), _ti as *const u8, size_of::<TaskInfo>());
    let task_info = get_current_task_info();
    let mut task_info_ptr = &task_info as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            task_info_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            task_info_ptr = task_info_ptr.add(buffer.len());
        }
    }
    0
}
```

整体思路和Q1一致，但这里我调用了task模块的get_current_task_info函数。这个函数解决ch3的问题时写过，但ch4的源码中并没有提供这个接口，因此按照ch3的思路重新写一下。

在syscall/process.rs中先导入这个函数。

```
use crate::task::get_current_task_info;
```

在task/mod.rs中完善这个函数。

```
/// get_current_task_info
pub fn get_current_task_info() -> (TaskStatus, [u32; MAX_SYSCALL_NUM], usize) {
    TASK_MANAGER.get_current_task_info()
}
```

~~可以看到调用了TASK_MANAGER实例的get_current_task_info方法，因此需要继续完善结构体task_manager的内联函数get_current_task_info的函数逻辑。也在task/mod.rs目录下。~~**注意注意这里有bug!!!!!!!!!!!!**或者说没有和之前的代码对应起来，后面测试的时候发现下面的这个函数返回的元组信息和sys_task_info的参数对应的TaskInfo的信息对不上，找了半天，发现我直接把元组的二进制数据复制到了TaskInfo中，当然不一样，giao，学了这么久居然吃到类型不一致的亏。

```
impl task_manager{
    fn get_current_task_info(&self) -> (TaskStatus, [u32; MAX_SYSCALL_NUM], usize) {
        let inner = self.inner.exclusive_access();
        let current:usize = inner.current_task;
        let task = &inner.tasks[current];//control block
        let current_time = get_time_ms();
        let time = current_time - task.start_time;
        (task.task_status, task.syscall_count, time)
    }
}
```

我的解决方式是在sys_task_info中把这个元组转换成一个TaskInfo类型。将其修改为

```
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    let buffers = translated_byte_buffer(current_user_token(), _ti as *const u8, size_of::<TaskInfo>());
    let task_info = get_current_task_info();
    let task_info = TaskInfo {
        status: task_info.0,
        syscall_times: task_info.1,
        time: task_info.2,
    };
    let mut task_info_ptr = &task_info as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            task_info_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            task_info_ptr = task_info_ptr.add(buffer.len());
        }
    }
    0
}
```



在TASK_MANAGER.get_current_task_info()种我们实际获取了当前函数的控制块结构体，目前ch4的该结构体定义如下。

```
pub struct TaskControlBlock {
    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,
}
```

我们需要获取当前任务的task_status, syscall_count, time，这里有几个小问题。

+ TaskControlBlock的定义中没有定义系统调用计数和第一次系统调用的时间。我们将其修改为：

```
pub struct TaskControlBlock {
    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,
    
    /// Syscall_times
    pub syscall_count: [u32; MAX_SYSCALL_NUM],//every syscall recorded
    
    /// First yield time
    pub start_time: usize,
    
}
```

+ 我们还没有重新实现系统调用计数的逻辑。这里我们修改syscall/mod.rs，每次调用时利用task::increase_current_syscall_count先计数，再进行系统调用。修改之后还需要在task/mod.rs完善task::increase_current_syscall_count的声明。

修改syscall/mod.rs。

```
pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    super::task::increase_current_syscall_count(syscall_id);//所有系统调用都需要计数
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_TASK_INFO => sys_task_info(args[0] as *mut TaskInfo),
        SYSCALL_MMAP => sys_mmap(args[0], args[1], args[2]),
        SYSCALL_MUNMAP => sys_munmap(args[0], args[1]),
        SYSCALL_SBRK => sys_sbrk(args[0] as i32),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
```

完善task::increase_current_syscall_count。

```
impl TaskManager {
    fn increase_current_syscall_count(&self, syscall_id: usize) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].syscall_count[syscall_id] += 1;
    }
}
/// increase_current_syscall_count
pub fn increase_current_syscall_count(syscall_id: usize) {
    if syscall_id >= MAX_SYSCALL_NUM {
        return;
    }
    TASK_MANAGER.increase_current_syscall_count(syscall_id);
}
```

+ TaskControlBlock的start_time代表第一次被调度的时刻，我们还需要对所以加入TaskManager的任务的TaskControlBlock进行初始化。原本的初始化代码在task/mod.rs中：

```
lazy_static! {
    /// a `TaskManager` global instance through lazy_static!
    pub static ref TASK_MANAGER: TaskManager = {
        println!("init TASK_MANAGER");
        let num_app = get_num_app();
        println!("num_app = {}", num_app);
        let mut tasks: Vec<TaskControlBlock> = Vec::new();
        for i in 0..num_app {
            tasks.push(TaskControlBlock::new(get_app_data(i), i));
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}
```

这里对应的初始化代码由TaskControlBlock的内联函数new提供。因此我们在task/task.rs的对应位置修改new函数。这个地方和第3章有些不一样。修改下面这些地方，然后导入timer的get_time_ms函数接口和config的MAX_SYSCALL_NUM。现在初始化和获取sys_task_info需要的时间应该不是问题了。**可能后面会提示一些包没有导包，这里避免啰嗦不记录了。**

```
        let syscalls = [0; MAX_SYSCALL_NUM];
        let starttime = get_time_ms();
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
            syscall_count: syscalls,
            start_time: starttime,
       	};
```

这里commit一下，因为前两个任务和后两个任务比较独立，前两个任务已经解决了。
# Q3 mmap 匿名映射

需要补写mmap系统调用

```
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize
```

主要功能是映射虚存文件到物理内存中，这个函数包含三个参数，_start代表虚存的起始地址， _len代表这块虚存的长度，_  _prot代表权限设置。**我的想法是在这个函数内部先检查参数合法性，然后调用task模块的一个task_mmap函数来实现主要功能，因为这个工作应当由TASK_MANAGER完成。**首先补写检查参数合法性的代码，然后把prot对权限的描述转换为permission。

```
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    if _start % 4096 != 0 { return -1; } //start没有对齐到页
    if _prot & !0x7 != 0 { return -1; } //prot除最后3位外其余的位不为0
    if _prot & 0x7 == 0 { return -1; } //prot的最后三位为0
    // 物理空间不足，暂时不处理
    let mut permission = MapPermission::from_bits((_prot as u8) << 1).unwrap();
    permission.set(MapPermission::U, true); //把prot转换为permission
    task_mmap_area(_start, _len, permission)
}
```

接下来在task/mod.rs中定义task_mmap_area函数，并在syscall/process.rs中导入。

```
//task/mod.rs
...
///task_mmap_area
pub fn task_mmap_area(_start:usize,_len:usize,permission:MapPermission)->isize{
    TASK_MANAGER.task_mmap_area(_start,_len,permission)
}

//syscall/process.rs
use crate::task::task_mmap_area;
```

我调用了TASK_MANAGER实例的一个方法，现在我们在Task_Manager的内联方法中补充它。首先我们需要获取到当前任务的MemorySet，然后检查需要映射的区间对应的所有页号有没有被映射，如果都没有的话，调用Memory的insert_framed_area方法来进行插入和映射。

+ 获取当前任务的token,根据token获取页表。把页表的查找接口设置为pub之后，直接调用。

```
use crate::mm::{PageTable，VirtPageNum};
...
//task_mmap_area
impl TaskManager{
    /// task_mmap_area
    fn task_mmap_area(&self,_start:usize,_len:usize,permission:MapPermission)->isize{
        let token = self.get_current_token();
        let page_table = PageTable::from_token(token);
        //虚拟页号
        let start_vpn = _start / 4096;
        let end_vpn = (_start + _len + 4095) / 4096;//最后一页的后一页的页号
        for vpn in start_vpn..end_vpn {
            let c_vpn = vpn as VirtPageNum;
            if page_table.find_pte(c_vpn).is_some(){//如果已经映射了
                return -1;
            }
        }
        -1
    }
}
```

+ 如果区间合法的话直接找到当前人物的MemorySet，调用insert_framed_area方法直接插入。注意有一行是减4096。

```
impl TaskManager{
    ///task_mmap_area
    fn task_mmap_area(&self,_start:usize,_len:usize,_permission:MapPermission)->isize{
        let token = self.get_current_token();
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
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &mut inner.tasks[current];
        let memory_set = &mut task.memory_set;
        //传入虚拟内存地址，之后调用的函数会自动转换为页号，向下向上取整
        let start_va = VirtAddr::from(_start);
        let end_va = VirtAddr::from(_start + _len-1);//注意这个地方是减4096还是减1
        memory_set.insert_framed_area(start_va,end_va, _permission);
        0
    }
}
```

截至目前，我们可以完成14个测试。**再commit一下。测试的时候发现，调用上一个写的mmap函数的的时候还是有问题，本来只映射一个页面，这个时候映射了两个。由于Q3已经大体解决了分配的问题，我们在Q4来解决这个Q3实现时的bug。下面这行代码是减1的时候，mmap其它的测试不会报错，但是munmap测试时会报错，重复分配连续空间65536和65537、63368时，会查到65537已经被映射。但是我改成减4096的时候，mmap相关的测试又会报错。**

# Q4 目前mmap的bug解释

首先我把代码先推到远程仓库进行最终的测试，调用需要通过的所有测试，看下问题究竟有多大。结果和本地测试差不多，所以远程仓库也应该利用本地拉的ci测试代码测试的。那接下来着手解决这个bug。

目前没过的测试为。

```
fn main() -> i32 {
    let start: usize = 0x10000000;
    let len: usize = 4096;
    let prot: usize = 3;
    assert_eq!(0, mmap(start, len, prot));
    assert_eq!(mmap(start + len, len * 2, prot), 0);
    assert_eq!(munmap(start, len), 0);
    assert_eq!(mmap(start - len, len + 1, prot), 0);
    for i in (start - len)..(start + len * 3) {
        let addr: *mut u8 = i as *mut u8;
        unsafe {
            *addr = i as u8;
        }
    }
    for i in (start - len)..(start + len * 3) {
        let addr: *mut u8 = i as *mut u8;
        unsafe {
            assert_eq!(*addr, i as u8);
        }
    }
    println!("Test 04_5 ummap OK15011!");
    0
}
```

目前报错在下面这段代码

```
    assert_eq!(0, mmap(start, len, prot));
    assert_eq!(mmap(start + len, len * 2, prot), 0);
```

首先分配start开始的len长的虚存，然后分配start+len开始的2len长的虚存即两个区间

+ 0x10000000-0x10001000    页号分别为0x10000和0x10001 65536 65537，显然这个地方我们只要分配65536
+ 0x10001000-0x10003000    页号分别为0x10001和0x10003 65537 65539，这个地方我们需要分配65537和65538

之前的测试我发现把区间的右端点的页号也分配了，也就是4096的len，我分配了两个页号65536和65537。现在我修改测试代码为，从65538开始分配，看这个位置的代码能否通过测试。答案是不能。另外我发现此时我把起始位置设置为start + 256*len也是返回-1，而start+512 *len却返回0。很奇怪，mmap(start, len, prot)的代码分配了这么大的空间吗。

1. 检查**(mmap(start + 256*len, len * 2, prot), 0)**为什么报错，**我把对应检查逻辑的代码返回值改成不同的值，以便查看原因。**

```
        let start_vpn = _start / 4096;
        let end_vpn = (_start + _len + 4095) / 4096;//最后一页的后一页的页号
        for vpn in start_vpn..end_vpn {
            let c_vpn:VirtPageNum = vpn.into();
            if page_table.find_pte(c_vpn).is_some(){//如果已经映射了
                return vpn as isize;
            }
        }
```

重复测试，观察到(mmap(start + 256*len, len * 2, prot), 0)的值为65792，也就是说这个虚拟页号已经被映射了。接下来我们看下它是什么时候被映射的。(mmap(start + len, len * 2, prot)应该也是类似的原因。

2. 检查映射代码

首先是在task模块中构造的Task_Manager的内联方法。

```
    fn task_mmap_area(&self,_start:usize,_len:usize,_permission:MapPermission)->isize{
        let token = self.get_current_token();
        let page_table = PageTable::from_token(token);
        //虚拟页号
        let start_vpn = _start / 4096;
        let end_vpn = (_start + _len + 4095) / 4096;//最后一页的后一页的页号
        for vpn in start_vpn..end_vpn {
            let c_vpn:VirtPageNum = vpn.into();
            if page_table.find_pte(c_vpn).is_some(){//如果已经映射了
                return vpn as isize;
            }
        }
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &mut inner.tasks[current];
        let memory_set = &mut task.memory_set;
        //传入虚拟内存地址，之后调用的函数会自动转换为页号，向下向上取整
        let start_va = VirtAddr::from(_start);
        let end_va = VirtAddr::from(_start + _len-1);//注意这个地方是暂时是减一，后面发现不用减
        //////////注意！！！！/////////////
        memory_set.insert_framed_area(start_va,end_va, _permission);
        0
    }
```

这个地方我们把_start _start+ _len-1转换成虚拟内存地址，然后转换为页号。虚拟内存地址会取二者的低39位作为结构体的字段。然后我们调用了memory_set.insert_framed_area(start_va,end_va, _permission);这个函数实际上调用了memory_set的push方法。

```
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }
```

memory的结构如下，一个页表和对应的已被映射的areas。

```
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}
```

再看看push

```
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }
```

由于insert_framed_area没有给push传数据，所以它做的主要的事情就是：新建一个maparea，然后把这个maparea调用map方法，映射到memory页表中，然后把这个maparea压到memory结构体中的areas数组中。

接下来我们需要专注MapArea的new和map方法，看它们是怎么映射的。

```
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
```

new方法把传入的两个虚拟地址分别向下和向上取整得到页号。然后创建一个新的maparea。先看看vpnrange的结构，再看map方法。

```
pub type VPNRange = SimpleRange<VirtPageNum>;
pub struct SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    l: T,
    r: T,
}
impl<T> SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    pub fn new(start: T, end: T) -> Self {
        assert!(start <= end, "start {:?} > end {:?}!", start, end);
        Self { l: start, r: end }
    }
    pub fn get_start(&self) -> T {
        self.l
    }
    pub fn get_end(&self) -> T {
        self.r
    }
}
```

这个结构体包含一个l和一个r，l为maparea输入的start_vpn,r为end_vpn。接下来看看map

```
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }
```

这个地方遍历实际是一个一个页面插入的，vpn_range中的T实现了迭代器。并且从下面的代码中可以看出，不会遍历页号右端点不会参与遍历。

为了查看究竟哪些被遍历， **我们这这个地方的vpn打印出来。**但是问题在于会很乱，其它很多位置也调用了这个函数。暂时我们先看MapArea的new方法收到了哪些vpn。打印后发现只有65536和65537，所以问题出在哪？？？？我怀疑是查询代码出问题了，可能查到别的页表了？

3. 检查查询代码

这个查询代码是不是可能有问题，首先这个查询代码肯定是查询的当前任务的页表，因为我修改分配情况和查询情况它返回的不一样。但是为啥呢？等等，查询页表项和查询页表项是否存在是一个意思吗？ **我修改了这个函数的顺序。真想骂人啊**爽

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



# Q4 munmap 解除映射

这个问题应该比上一个问题简单，我们需要先获取当前任务的MemorySet，然后查看我们要接触映射的虚拟内存区间是否在areas里面，如果没在就返回-1，在的话需要删除对应的页表和area。同样的我们在task/mod.rs中构造接口，然后在syscall/process.rs中调用。

```
//syscall/process.rs
...
use crate::task:;task_munmap_area;
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap");
    task_munmap_area(_start, _len)
}
//task/mod.rs
...
impl TaskManager{
    ///task_munmap_area
    fn task_munmap_area(_start:usize,_end:usize)->isize{
       //待做
    }
}
///task_munmap_area
pub fn task_munmap_area(_start:usize,_len:usize)->isize{
    TASK_MANAGER.task_munmap_area(_start,_len)
}
```

+ 获取当前任务的MemorySet，然后查看我们要接触映射的虚拟内存区间是否在areas里面。~~这里实在有点懒了，areas和每个area的vpn_range字段是私有的，本来应该写接口暴露出来的，现在直接把它改成pub。这样我在task/mod.rs里面就可以直接访问它了，但是操作系统实际这么操作是不规范的。解除映射首先需要操作页表，然后删除area。~~

```
    fn task_munmap_area(&self,_start:usize,_end:usize)->isize{
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        let task = &mut inner.tasks[current];//控制块
        let memory_set = &mut task.memory_set;//其areas字段私有，只能通过控制块访问,修改为pub
        let mut area_id = 0;
        let page_table =&mut memory_set.page_table;
        for area in memory_set.areas.iter_mut(){
            if area.start_vpn.into() == _start/4096 && area.end_vpn.into() == _end/4096{
                area.unmap(page_table);
                memory_set.areas.remove(area_id);
                return 0;
            }
            area_id += 1;
        }
        -1
    }
```

出问题了
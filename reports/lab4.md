

这章的代码同样需要向前兼容，通过前面几章的测例。具体为：

```
static TESTS: &[&str] = &[
    "ch2b_hello_world\0",
    "ch2b_power_3\0",
    "ch2b_power_5\0",
    "ch2b_power_7\0",
    "ch3b_yield0\0",
    "ch3b_yield1\0",
    "ch3b_yield2\0",
    "ch3_sleep\0",
    "ch3_sleep1\0",
    "ch4b_sbrk\0",
    "ch4_mmap0\0",
    "ch4_mmap1\0",
    "ch4_mmap2\0",
    "ch4_mmap3\0",
    "ch4_unmap\0",
    "ch4_unmap2\0",
    "ch5b_exit\0",
    "ch5b_forktest_simple\0",
    "ch5b_forktest\0",
    "ch5b_forktest2\0",
    "ch5_spawn0\0",
    "ch5_spawn1\0",
    "ch6b_filetest_simple\0",
    "ch6_file0\0",
    "ch6_file1\0",
    "ch6_file2\0",
    "ch6_file3\0",
];
```

首先在ci-user目录下`make test CHAPTER`，可以看到几乎所有测试例子都不能通过。这是因为这个测试的主函数中使用了spawn来启动所有进程，而sys_spawn系统调用需要我们自己实现，这些测例的可执行文件都放在文件系统中，和上一章直接在内存的内核数据段读取数据不同，这一章我们需要从文件系统中读取可执行文件。值得高兴的是，我们使用spawn直接调用实验中提供好的fork和exec函数，而exec函数对于这一变化已经适配好了，我们只需要按照上一章的思路实现spawn即可。



# ch3-ch5的syscall

首先实现spawn，这里参考ch6的sys_exec调用，修改获取可执行文件的方法。

```
/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(app_inode) = open_file(path.as_str(),OpenFlags::RDONLY) {
        let data = app_inode.read_all();
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

接下来实现TCB的spawn方法。

```
impl TaskControlBlock{
    pub fn spawn(self: &Arc<Self>, elf_data: &[u8]) -> Arc<Self> {
        let new_task = self.fork();
        new_task.exec(elf_data);
        new_task
    }
}
```

重新测试，可以看到每个进程成功创建，获取到了进程id，并且一些测试已经成功通过。由于ch2-ch5的其他全部测例，不会涉及到对于文件系统的操作，因此我们按照ch5的实现方法，分别实现sys_get_time、sys_mmap、sys_munmap、sys_set_priority。按照ch5实现sys_mmap和sys_mumap之后，只有一下几个测试（ch6-file1~3）没有通过。注意这个ch6中没有对于stride算法的测试，减少了工作量。

```
[FAIL] not found <Test fstat OK6110061040!>
[FAIL] not found <Test link OK6110061040!>
[FAIL] not found <Test mass open/unlink OK6110061040!>
```

接下来需要实现syscall/fs.rs中的三个系统调用sys_linkat、sys_unlinkat、sys_stat。

# sys_linkat

当创建一个硬链接时，系统会创建一个新的文件名，这个新文件名指向与原始文件相同的 inode。因此我们需要:

1. 判断原来的文件是否存在。
2. 找到原来的文件的inode。
3. 创建新的文件，但是指向原来的文件的inode。

事实上，这个过程可以借鉴easy-fs/src/vfs.rs的find和creat接口。接下来是具体的实现。首先补充sys_link的内容，把两个文件名转换成字符串slice之后，直接调用fs模块的linkat接口。

```
/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let token = current_user_token();
    let old_name_path = translated_str(token, _old_name);
    let new_name_path = translated_str(token, _new_name);
    linkat(_old_name.as_str(), _new_name.as_str())
}
```

引入linkat接口

```
use crate::fs::linkat;
```

在fs/mod.rs中暴露接口

```
pub use inode::linkat;
```

在fs/inode.rs中添加linkat函数，直接调用ROOT_INODE的linkat方法。

```
/// crate a Hard Link
pub fn linkat(old_name:&str,new_name:&str)->isize{
    ROOT_INODE.linkat(old_name,new_name)
}
```

在easy-fs/src/vfs.rs中给ROOT_INODE添加linkat方法，并标记为pub。

```
    /// Create a hard link in current inode
    pub fn linkat(&self, old_name:&str,new_name:&str)->isize{
        
    }
```

接下来在这个linkat中实现具体的逻辑。这里就参考去年的答案了，写的比较好。

```
    pub fn linkat(&self, old_name:&str,new_name:&str)->isize{
        let mut fs = self.fs.lock();
        //找到原文件的inode没有的话返回-1
        let old_inode_id =
            self.read_disk_inode(|root_inode| self.find_inode_id(old_name, root_inode));
        if old_inode_id.is_none() {
            return -1;
        }

        //找到原文件的inode位置
        let (block_id, block_offset) = fs.get_disk_inode_pos(old_inode_id.unwrap());

        // Get the target DiskInode according to `block_id` and `block_offset`
        get_block_cache(block_id as usize, Arc::clone(&self.block_device))
            .lock()
            // Increase the `nlink` of target DiskInode
            .modify(block_offset, |n: &mut DiskInode| n.nlinks += 1);

        // Insert `newname` into directory.
        self.modify_disk_inode(|root_inode| {
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_inode, &mut fs);
            let dirent = DirEntry::new(new_name, old_inode_id.unwrap());
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        //擦除缓存
        block_cache_sync_all();
        0
    }
```

这里我们还需要给DiskInode添加一个nlink，代表这个被硬链接的次数。

```
pub struct DiskInode {
    pub size: u32,
    pub direct: [u32; INODE_DIRECT_COUNT],
    pub indirect1: u32,
    pub indirect2: u32,
    pub nlinks: u32,
    type_: DiskInodeType,
}
```

修改初始化代码，这里把其初始的值设置为1，相比为0的情况，这样我们在创建inode的时候就不用更新的。只是创建硬链接的时候需要+1。

```
impl DiskInode {
    /// Initialize a disk inode, as well as all direct inodes under it
    /// indirect1 and indirect2 block are allocated only when they are needed
    pub fn initialize(&mut self, type_: DiskInodeType) {
        self.size = 0;
        self.direct.iter_mut().for_each(|v| *v = 0);
        self.indirect1 = 0;
        self.indirect2 = 0;
        self.nlinks = 1;
        self.type_ = type_;
    }
}    
```

现在剩下的三个测例还是通不过，因为都调用了stat。接下来先实现sys_stat，再实现sys_unlink.

# sys_stat

获取每个文件的状态，可以给file trait定义一个新的方法，所有的文件都需要实现这个方法的接口，就可以了。首先我们补全sys_stat。直接调用fs模块的fstat接口。我在类型转换的时候遇到了问题，所以并没有使用mm提供的translated_refmut，而是借用了它的内部逻辑。

```
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat",
        current_task().unwrap().pid.0
    );
    //根据文件描述符获取当前的OSInode
    println!("sys_fstat fd");
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    let os_inode = inner.fd_table[_fd].as_ref().unwrap();
    let page_table = PageTable::from_token(token);
    let va = _st as usize;
    let mut stat = page_table
        .translate_va(VirtAddr::from(va))
        .unwrap()
        .get_mut();
    let mut path = &mut stat;
    os_inode.stat(path)
}
```

在fs/mod.rs中中给file trait添加stat方法

```
pub trait File: Send + Sync {
    /// the file readable?
    fn readable(&self) -> bool;
    /// the file writable?
    fn writable(&self) -> bool;
    /// read from the file to buf, return the number of bytes read
    fn read(&self, buf: UserBuffer) -> usize;
    /// write to the file from buf, return the number of bytes written
    fn write(&self, buf: UserBuffer) -> usize;
    /// stat the file
    fn stat(&self, st: &mut Stat) -> isize;
}
```

然后我们给OsInode实现stat方法，之后把这个方法封装成fstat函数，暴露出来。

```
//重新写一个DiskInodeType

impl File for OSInode {
    fn stat(&self, st: &mut Stat) -> isize {
        let inner = self.inner.exclusive_access();
        inner.inode.read_disk_inode(|disk_inode| {
            st.mode = match disk_inode.type_ {
                DiskInodeType::File => StatMode::FILE,
                DiskInodeType::Directory => StatMode::DIR,
            };
            st.nlink = disk_inode.nlinks;
        });
        0
    }
}
```

还要在src/fs/stdio.rs给stdio实现这个方法。应该没有相关的测试，暂时实现通通给一个0。

```
impl File for Stdin{
    fn stat(&self, st: &mut Stat) -> isize {
    0
    }
}
impl File for Stdout {
    fn stat(&self, st: &mut Stat) -> isize {
        0
    }
}
```

接下来需要调整一系列的参数为pub，添加文档。

# sys_unlinkat

和sys_link的实现思路类似，这里就不写了。

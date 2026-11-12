# Lab 4：文件状态、硬链接与文件生命周期

账号：hilkyhzq；分支：ch6；日期：2026-11-12。

## 目标与环境

实现 fstat、linkat、unlinkat，并保留此前章节的时间、匿名映射、堆调整、spawn 和优先级调度。工具链、QEMU、固件和固定官方检查器版本与前述报告相同。

## inode 与目录项

文件内容由 inode 管理，文件名只是目录中的“名字→inode 编号”映射。硬链接增加目录项而不复制数据，故不同名字的 fstat 应得到相同 dev/ino。删除一个名字时，其他名字仍能访问同一内容。

实现保持磁盘格式不变，根据 inode 块号与块内偏移反算稳定 inode 编号；链接数通过扫描有效根目录项计算。删除的目录项置为空，创建文件或新增硬链接时优先重用空槽，ls 和名字查找忽略这些空槽。根目录的数据块保持已分配容量供后续复用，当前实现不回缩目录块。

文件系统仍是课程的单层根目录模型。新链接名必须非空、不包含斜杠，且不超过目录项可容纳的 27 字节；重复目标名、缺失源文件、无效名字均失败，不改变目录。

## 打开文件与解除链接

目录链接数与打开的文件描述不同。删除最后一个文件名后，已经打开的描述仍应访问原 inode，直到最后一个打开描述释放后才可回收其数据块及 inode 位图位。

OSInode 创建时登记 inode 的打开描述计数，Drop 时减少；fork/FD 共享只增加 Arc 引用，不增加描述计数，直到所有共享该描述的引用消失才触发 Drop。unlink 删除目录项后，仅在链接数与打开描述计数都为零时回收。若仍被打开，则由最后一个 OSInode 的 Drop 执行回收。同名新文件会分配不同的活动 inode，不覆盖旧句柄的数据。

## fstat 和用户缓冲区

File trait 提供可选的 stat 方法，普通文件返回 dev、ino、普通文件类型、当前 nlink 及清零的保留字段。没有实现此能力的设备返回不支持。sys_fstat 先验证 FD 并克隆文件 Arc，释放任务借用后获取状态，再检查用户缓冲区的全部可写页面，按字节跨页复制，避免用户指针未对齐或跨页时直接写结构体。

link/unlink 的路径同样通过 checked_user_string 逐页验证、检查 NUL 终止和 UTF-8，而非对用户地址直接解引用。

## 验证记录

原始分支为 `2/31`。移植此前进程/内存实现后得到 `28/31`，剩下的失败来自 fstat、硬链接和批量解除链接。完成文件系统实现后得到 `31/31`，包括相同 inode 多名字访问、链接数从 1 增至 4 再减少，以及 10 轮大量写入后关闭、解除链接、确认名字消失。首次得到 31/31 的命令仍因缺少 lab4 报告退出，补齐报告后重新进行完整验证。

```sh
PATH=/home/hilky/rcore-tools/qemu8/bin:$PATH make -C ci-user test CHAPTER=6
```

额外的打开句柄/解除链接回归在独立验证副本中运行，正式检查器和正式验证的测试源保持固定版本。代码、环境及报告使用编程助手辅助。该教学文件系统没有日志恢复机制，不宣称提供断电事务保证。

额外验证副本实际得到官方 31/31、额外完成标记 `Extra_unlink_regression_complete`、四份报告齐全且检查器退出码 0。该程序还覆盖重复链接拒绝、未找到源文件、旧文件仍打开时同名新建、旧数据保持可读、100 次创建/解除链接回收循环及无效 FD。其源程序如下：

```rust
#![no_std]
#![no_main]
extern crate user_lib;
use user_lib::{close, fstat, link, open, read, unlink, write, println, OpenFlags, Stat};

#[no_mangle]
fn main() -> i32 {
    let name = "extra_orphan\0";
    let alias = "extra_alias\0";
    let a = open(name, OpenFlags::CREATE | OpenFlags::RDWR) as usize;
    assert_eq!(write(a, b"old"), 3);
    let b = open(name, OpenFlags::RDONLY) as usize;
    assert_eq!(link(name, alias), 0);
    assert_eq!(link(name, alias), -1);
    assert_eq!(link("absent_extra\0", "absent_alias\0"), -1);
    assert_eq!(unlink(name), 0);
    assert_eq!(unlink(alias), 0);
    let mut old = Stat::new();
    assert_eq!(fstat(a, &mut old), 0);
    assert_eq!(old.nlink, 0);
    assert_eq!(close(a), 0);
    let c = open(name, OpenFlags::CREATE | OpenFlags::RDWR) as usize;
    assert_eq!(write(c, b"new"), 3);
    let mut new = Stat::new();
    assert_eq!(fstat(c, &mut new), 0);
    assert_ne!(old.ino, new.ino);
    let mut data = [0u8; 3];
    assert_eq!(read(b, &mut data), 3);
    assert_eq!(&data, b"old");
    assert_eq!(close(b), 0);
    assert_eq!(close(c), 0);
    assert_eq!(unlink(name), 0);
    assert_eq!(unlink(name), -1);
    for _ in 0..100 {
        let fd = open(name, OpenFlags::CREATE | OpenFlags::WRONLY) as usize;
        assert_eq!(write(fd, b"reuse"), 5);
        assert_eq!(close(fd), 0);
        assert_eq!(unlink(name), 0);
    }
    assert_eq!(fstat(9999, &mut new), -1);
    println!("Extra_unlink_regression_complete");
    0
}
```

后续复查为 spawn 补充静态 ELF64/RISC-V 验证，拒绝空文件、普通文本、截断头和越界段，防止把无效文件交给会断言的教学装载器。验证模块同时供第八章使用。

新增 spawn 回归实际通过：空文件、普通文本、仅有 ELF 前缀的截断文件均返回 -1，正常 ch5_getpid 仍成功创建且可回收。扩展测试副本同时保持官方 31/31、四份报告检查通过和退出码 0。

# 第五章实验报告

分支：`ch5`　日期：2026 年 11 月 9 日

## 完成的功能

这次补全了 `spawn`、`sys_set_priority` 和 stride 调度，把上一章的 `get_time`、`mmap`、`munmap` 迁移到新的进程结构。新进程的优先级默认为 16，stride 从 0 开始；设置小于 2 的优先级返回 `-1`。另外修正了 `exec` 后堆边界没有重置的问题。

## 实现过程

一开始看 `spawn` 时，我选的是 `fork`，因为觉得创建子进程就应该由父进程管理。但看了地址空间的创建过程后，发现 `fork` 会复制父进程的内存。父进程运行 A，`spawn` 要启动 B，复制 A 的内容就多余了。这里改用 `TaskControlBlock::new`，直接从 B 的 ELF 建立地址空间。

我还以为子进程的内存要挨着父进程。其实各进程有自己的页表，物理页不需要相邻。父子关系记在 PCB 里：父进程的 `children` 用 `Arc` 保存孩子，孩子的 `parent` 用 `Weak` 指向父进程，避免强引用环。

调度时扫描就绪队列，找 stride 最小的进程，同值时选先入队的那个。选中后给它增加 `BIG_STRIDE / priority`。

`BIG_STRIDE` 取 `2^64`，stride 用 `u128` 保存，即使优先级设成 `isize::MAX`，步长也不会变成 0。累加将溢出时，给所有就绪进程的 stride 减去同一个最小值，大小关系不变，也腾出了累加空间。当前这一章没有阻塞队列，取任务时所有可运行进程都在就绪队列中。

上一章的内存管理代码主要改了访问入口：从旧的任务数组改为当前进程的 `memory_set`。另外，`exec` 换掉地址空间后，需要一起重置 `heap_bottom` 和 `program_brk`，避免新程序沿用旧的堆边界。

## 测试

测试使用 `nightly-2024-05-02`、QEMU 8.2.2 和 RustSBI 0.4.1。用户程序在 Docker 中编译，内核在 macOS 上交叉编译，再交给容器里的 QEMU 运行。

加载 `BASE=2` 的测例后，在 shell 中分别运行 `ch5_usertest` 和 `ch5b_usertest`。保存输出，再执行：

```sh
python3 ci-user/check/ch5.py < tmp/ch5-validation-20261109/ch5_usertest.log
python3 ci-user/check/ch5b.py < tmp/ch5-validation-20261109/ch5b_usertest.log
```

两次输出检查分别是 **15/15** 和 **17/17**，表示检查项数。两个内存权限测例按预期退出 `-2`，其余 ch5 子测例退出 0。

以上是官方测例的本地运行结果，原始输出和检查结果保存在 `tmp/ch5-validation-20261109/`。

## 问答题

### 1. 溢出后会轮到 p1 吗

不会。按 8 位整数回绕，p2 执行后存下来的值是 `(250 + 10) % 256 = 4`。普通无符号比较认为 4 比 255 小，所以还会选 p2。

如果能保存完整的累计值，比较的应该是 260 和 255，下一次才会选 p1。

### 2. 为什么 stride 的差值不会超过 BigStride 的一半

令 `D = BigStride / 2`，按整数除法取整。因为优先级至少为 2，每次增加的步长最多是 D。

初始 stride 全为 0，差值也是 0。假设调度前所有值都在 `[m, m + D]` 内，m 是最小值。只给 m 加一个不超过 D 的数，结果仍在这个区间里，其他值没变。因此新的最大值不超过 `m + D`，最小值不低于 m，差值还是不超过 D。这样每调度一次，都能保持这个范围。

这里假定是同一批从 0 开始的进程。运行中再加入一个 stride 为 0 的进程，不一定还满足这个范围。

### 3. 怎样让 BinaryHeap 弹出真正最小的 stride

假设真实累计值之间相差不到半圈，也就是小于 `2^63`。用 `wrapping_sub` 做减法，再把结果看成 `i64`，负数表示自己在逻辑上更小。

但 [BinaryHeap 默认是最大堆](https://doc.rust-lang.org/alloc/collections/binary_heap/struct.BinaryHeap.html)，所以交给堆的比较结果还要反过来。代码如下，补上 `Ord` 和 `Eq` 后可以直接用于 `BinaryHeap<Stride>`：

```rust
use core::cmp::Ordering;

#[derive(Eq, PartialEq)]
struct Stride(u64);

impl Ord for Stride {
    fn cmp(&self, other: &Self) -> Ordering {
        let logical = (self.0.wrapping_sub(other.0) as i64).cmp(&0);
        logical.reverse()
    }
}

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
```

题目的 8 位例子也是这样：`125 - 255` 回绕为 126，转成 `i8` 还是正数，所以逻辑上的 `125 < 255` 为假；`129 - 255` 回绕为 130，转成 `i8` 是 -126，所以逻辑上的 `129 < 255` 为真。

题面的 `<` 表示逻辑顺序；上面的类型为适配最大堆反转了排序，所以 `Stride` 自己的 `<` 也会反转。

比较时，所有真实值必须处在宽度小于半圈的同一区间内；恰好相差半圈就无法判断。题面的 `eq` 恒为 false，这里换成了正常的相等判断。这是问答的 u64 方案，内核实际用的是 `u128` 累计和减去最小值。

## 交流和参考资料

按[课程荣誉准则](https://learningos.cn/rCore-Tutorial-Guide/honorcode.html)，说明本次使用的帮助和资料：

1. 和 AI 助手讨论过 `spawn` 与 `fork`、父子进程的引用关系、页表和 `Arc`。路径读取、程序查找部分先按提示写入，后续实现、调试、测试和报告整理也使用了 AI 辅l助，对应代码中已注明。
2. 参考了课程模板、[第五章练习](https://learningos.cn/rCore-Tutorial-Guide/chapter5/4exercise.html)、[仓库 README](../README.md)、官方用户测例及上面链接的 Rust 文档。时间查询和内存映射沿用本人 `ch4` 分支的代码。
3. 除上面列明的交流、参考和辅助部分外，其余工作独立完成。我理解这些帮助降低了实验难度，可能影响起评分。
4. 代码仅用于课程实验和评测，按课程要求保管，不向其他学员提供作业答案，也不用于干扰或破坏系统。

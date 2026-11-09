# 第三章实验报告

分支：`ch3`　日期：2026 年 11 月 2 日

## 完成的功能

这次补全了 `sys_trace`。请求 0 从指定地址读一个字节；请求 1 把 `data` 的最低一个字节写到指定地址，成功后返回 0；请求 2 查询当前任务调用某个系统调用的次数。其他请求返回 -1。调用次数按任务分别保存，每次进入 `syscall()` 就先加一，所以查询 `sys_trace` 自己的次数时，也算上当前这一次。

## 实现过程

先做的是调用次数统计。我一开始觉得所有任务共用一份计数可以省空间，但这样会把不同任务的次数混在一起。例如 A 调用了三次、B 调用了一次，只存总数 4 就没法分别回答它们。最后是在每个 `TaskControlBlock` 里加一个数组，创建任务时全部置零。

计数的位置也想错过。我开始想等调用 `sys_trace` 时再统计，但之前发生的 `write` 等调用就漏掉了。后来把加一放到 `syscall()` 的 `match` 前面，查询时只负责读出已有的次数。

读写内存时，我把“取地址”和“读取地址里的数据”混淆过。这里传进来的 `_id` 是地址，转换后的 `ptr` 还是指向这个位置，`*ptr` 才是该位置里的字节。写入时则把 `*ptr` 放在赋值号左边，把要写的 `_data` 转成 `u8` 放在右边。

Rust 语法上也卡了几次。定义数组字段时，我把类型和初始化值写混，还加了 `vec!`。另一次是读取次数的包装函数，虽然能编译，但没写返回类型，调用后面又加了分号，读到的结果就没有返回出来。后来补上返回类型，并用最后一个表达式返回结果。

目前数组的有效下标是 0 到 410，超出范围的查询还没有处理。内存读写也没有检查地址是否合法，这里按本章要求先完成了测例中的功能。

## 测试结果

使用仓库的 Docker 环境，Rust 工具链为 `nightly-2024-05-02`，目标平台为 `riscv64gc-unknown-none-elf`，QEMU 为 8.2.2，RustSBI 为 0.4.1。

在容器内的仓库根目录执行：

```bash
make -C ci-user test CHAPTER=3
```

测试是随着功能逐步补上的。最初计数部分通过后，程序停在读取 `111` 的断言上；加上读取分支后，又停在写入后的检查上。写入分支补好后，`ch3_trace` 输出了 `Test trace OK!`。

第一次完整评测是 7/7，但检查器提示缺少报告。补上报告后又跑了一次，仍然是 7/7，同时输出 `Report for lab1 found.`，命令退出状态为 0。

## 问答题

以下回答对应本仓库的 [`trap.S`](../os/src/trap/trap.S)、[`TrapContext`](../os/src/trap/context.rs) 和 [`trap_handler`](../os/src/trap/mod.rs)。

### 1. 三个 bad 测例的运行结果

在 RustSBI 0.4.1 下，运行结果如下：

- `ch2b_bad_address` 向地址 0 写数据，内核打印 `PageFault in application`，错误地址是 `0x0`，随后结束这个任务。
- `ch2b_bad_instructions` 在 U 态执行 `sret`，内核打印 `IllegalInstruction in application`，随后结束这个任务。
- `ch2b_bad_register` 在 U 态读取 `sstatus`，也触发了非法指令异常，任务被结束。

这三个程序退出后，其他任务继续运行。这里的 `PageFault` 是内核打印的文字：代码把存储访问错误和存储缺页错误放在同一个分支处理，具体异常仍要看 `scause`。

### 2.1 刚进入 `__restore` 时，`sp` 指向哪里？什么情况下会用到它？

`sp` 指向当前任务内核栈上的 `TrapContext` 起始位置，后面的指令从这里取出保存的寄存器值。

应用第一次运行时，使用内核预先准备好的上下文，通过 `__restore` 进入用户态。应用发生 Trap 后，也会在内核处理结束时走到这里，恢复之前保存的上下文。

### 2.2 恢复 `sstatus`、`sepc` 和 `sscratch` 有什么用？

- `sstatus` 保存处理器状态，其中 `SPP` 决定返回时的特权级。本实验返回用户程序时，`SPP` 是 0。
- `sepc` 是恢复执行的位置。第一次启动时指向应用入口，系统调用返回时指向 `ecall` 后面的指令。
- `sscratch` 在这里先保存用户栈指针，等其他寄存器恢复完，再和 `sp` 交换。

寄存器含义参考了 [RISC-V 特权规范](https://docs.riscv.org/reference/isa/priv/supervisor.html)。

### 3. 为什么恢复寄存器时跳过 `x2` 和 `x4`？

`x2` 就是 `sp`。这时还要靠它读取内核栈上的上下文，所以用户栈指针先放在 `sscratch` 中，最后再恢复到 `sp`。

`x4` 是 `tp`。本章应用没有使用它，框架也就省略了对它的保存和恢复。

### 4. `__restore` 最后交换 `sp` 和 `sscratch` 后，它们各是什么？

`sp` 变成用户栈指针，`sscratch` 保存内核栈指针。此时内核栈上的 `TrapContext` 空间已经释放，下次发生 Trap 时可以再次使用内核栈。

### 5. 在哪条指令返回 U 态？

在 `sret`。它根据 `sstatus.SPP` 切换特权级，并跳到 `sepc` 指定的位置。本实验返回用户程序时 `SPP` 为 0，所以进入 U 态。

### 6. `__alltraps` 开头交换后，`sp` 和 `sscratch` 各是什么？

交换前，`sp` 指向用户栈，`sscratch` 保存内核栈指针。执行 `csrrw sp, sscratch, sp` 后，`sp` 指向内核栈，原来的用户栈指针暂存在 `sscratch` 中，接着就在内核栈上保存上下文。

### 7. 从 U 态进入 S 态由哪条指令触发？

对于系统调用，是用户程序中的 `ecall`。它触发来自 U 态的环境调用异常，硬件进入 S 态，跳到 `stvec` 指定的入口。所以执行到 `__alltraps` 时已经处于 S 态，里面的 `csrrw` 只是交换栈指针。

除了系统调用，用户态发生的其他异常和定时器中断也可以让程序进入内核。

## 交流和参考资料

按[课程荣誉准则](https://learningos.cn/rCore-Camp-Guide-2025S/honorcode.html)，说明本次使用的帮助和资料：

1. 我使用了AI辅助学习，讨论过计数应该放在哪里、数组怎么初始化、函数怎么返回结果，以及指针怎么读写。AI帮忙解释报错、检查代码和运行测试。
2. 用到的资料包括课程模板、README、用户测例、实验指导和 RISC-V 规范，具体链接列在下面。
3. 新增功能代码是我根据讨论和提示，在编辑器中逐步写入的。课程模板和 AI 辅助的部分已经在这里说明，代码中也补充了对应的来源注释。
4. 代码用于本课程的实验和评测，按课程要求保管，不向其他学员提供作业答案，也不用于干扰或破坏系统。

参考资料：

- [本仓库 README](../README.md)。
- [LearningOS 第三章练习](https://learningos.cn/rCore-Camp-Guide-2025S/chapter3/5exercise.html)：参考的是 2025S 版页面，`sys_trace` 接口与当前仓库一致。
- [RISC-V Supervisor-Level ISA](https://docs.riscv.org/reference/isa/priv/supervisor.html)。
- 本仓库的 [`task`](../os/src/task/mod.rs)、[`syscall`](../os/src/syscall/mod.rs)、[`sys_trace`](../os/src/syscall/process.rs) 和 [`ch3_trace` 测例](../user/src/bin/ch3_trace.rs)。

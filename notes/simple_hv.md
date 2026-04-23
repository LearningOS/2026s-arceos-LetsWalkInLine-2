# simple_hv 练习笔记

## 1) 练习目标

本次 `simple_hv` 的通过目标非常明确：

- guest 最终触发 `SbiMessage::Reset`；
- 在 HS trap 处理中保证 `a0 == 0x6688`、`a1 == 0x1234`；
- 打印 `Shutdown vm normally!`。

对应代码位置：`arceos/exercises/simple_hv/src/main.rs`。


## 2) 初始实现与问题

最开始的实现思路是：

- `IllegalInstruction` 里直接写 `A0 = 0x6688`，`sepc += 4`；
- `LoadGuestPageFault` 里直接写 `A1 = 0x1234`，`sepc += 4`。

这个实现能跑通部分流程，但语义上有两个风险：

1. **寄存器语义不匹配**：
   guest `_start` 中是 `csrr a1, mhartid` 和 `ld a0, 64(zero)`，所以更自然的是：
   - 非法指令分支影响 `a1`；
   - load fault 分支影响 `a0`。

2. **无条件前进 `sepc` 不规范**：
   只有在“确实完成这条指令语义（仿真成功）”时才应该推进 `sepc`。


## 3) 最终实现总览

在 `main.rs` 中完成了“最小规范 + 可通过测试”的实现：

- 新增常量：
  - `EMULATED_ILLEGAL_RET = 0x1234`
  - `EMULATED_LOAD_RET = 0x6688`
  - `EXPECTED_FAULT_LOAD_ADDR = 0x40`
- 新增辅助函数：
  - `read_htval()` / `read_htinst()`：读取 H 扩展 trap 辅助 CSR
  - `inst_len(insn_bits)`：根据编码最低两位判断 2/4 字节指令长度
  - `decode_csrr_mhartid(insn_bits)`：只识别 `csrr rd, mhartid` 这一类指令


## 4) Trap 处理设计

### 4.1 `VirtualSupervisorEnvCall`

- 保留原逻辑：在 `Reset` 分支读取 `a0/a1` 并断言。
- 断言：
  - `a0 == 0x6688`
  - `a1 == 0x1234`
- 通过后打印 `Shutdown vm normally!` 并返回 `true` 结束循环。

### 4.2 `IllegalInstruction`

- 从 `stval` 取出 `bad_insn`；
- 仅当 `decode_csrr_mhartid(bad_insn)` 成功时，执行仿真：
  - 把目标寄存器 `rd` 写为 `0x1234`（本题里是 `a1`）；
  - 用 `inst_len(bad_insn)` 推进 `sepc`；
- 若不是这条已知指令，直接 `panic`，避免误吞未知异常。

### 4.3 `LoadGuestPageFault`

- 读取并打印 `stval/htval/htinst`；
- 仅当 `stval == 0x40`（对应 `ld a0, 64(zero)`）时：
  - 写 `a0 = 0x6688`；
  - `sepc += 4`（该 `ld` 是 32-bit 指令）；
- 其他地址 fault 直接 `panic`。


## 5) 为什么这么设计

核心原则是：

- **只对已识别、可仿真的 trap 前进 `sepc`**；
- **未知情况快速失败（panic）**，方便定位问题；
- **严格围绕本题最小场景**，避免在只改 `main.rs` 的约束下引入大改。

这能在不扩展完整二阶段缺页处理框架的情况下，稳定达到 exercise 目标。


## 6) 本题 guest 指令链路对应关系

guest `_start` 关键指令：

1. `csrr a1, mhartid` -> `IllegalInstruction` -> 仿真写 `a1 = 0x1234`
2. `ld a0, 64(zero)` -> `LoadGuestPageFault` -> 仿真写 `a0 = 0x6688`
3. `ecall` -> `VirtualSupervisorEnvCall` -> 断言通过，正常 shutdown


## 7) 问题与讲解整理

### Q1. `stval` 里的 `bad_insn` 是什么？为什么会在 `stval` 里？

- `stval` 是 trap 附加信息寄存器，不同异常类型含义不同。
- 对 `IllegalInstruction`，实现常会把“导致异常的指令编码”放到 `stval`（也可能是 0，取决于硬件实现）。
- 启用 H 扩展后，`stval` 仍是附加信息槽位；当是 guest 地址相关异常时，`stval` 也常表示 guest virtual address。

### Q2. 能否通过 `bad_insn` 解析具体指令？怎么做？

- 可以，按 RISC-V 编码字段切片：`opcode/rd/funct3/rs1/csr`。
- 本实现只识别：
  - `opcode == 0x73`（SYSTEM）
  - `funct3 == 010`（CSRRS）
  - `rs1 == 0`
  - `csr == 0xF14`（`mhartid`）
- 全匹配才认为是 `csrr rd, mhartid` 并执行仿真。

### Q3. `htinst` 是什么？

- `htinst` 是 H 扩展提供的 trap 指令辅助 CSR（HS 处理 guest trap 时使用）。
- 用于帮助软件仿真/诊断，但不保证一定等于完整原始指令。
- 实际工程里通常作为辅助信息，与 `scause/stval/htval` 联合使用。

### Q4. `inst_len` 为什么能从 `insn_bits` 判断长度？`#[inline]` 是什么？

- RISC-V 指令长度判定规则：最低两位为 `11` 时至少 32-bit，否则通常是 16-bit 压缩指令。
- 本题只需区分 2/4 字节，因此该规则足够。
- `#[inline]` 是 Rust 给编译器的内联优化提示，不改变语义。

### Q5. 和 rCore 里 `sstatus/sepc/scause/stval/stvec` 的关系是什么？

- 这五个寄存器在 H 扩展场景下依然是 trap 处理核心。
- 差别在于 trap 来源更复杂（可能来自 guest），所以还会配合 `htval/htinst` 使用。
- 可以理解为：
  - `scause` 定位类型，
  - `sepc` 定位 PC，
  - `stval` 给附加信息，
  - `htval/htinst` 提供 guest trap 的补充上下文。


## 8) 当前实现边界

当前版本是教学/练习导向的最小实现，不是完整通用 hypervisor：

- 没有实现通用非法指令仿真框架；
- 没有实现二阶段缺页修复后重试（`map + hfence + 不改 sepc`）全流程；
- 对未知 trap 分支选择 `panic`，优先确保可观测性与可调试性。


## 9) 后续可演进方向

后续若要升级为更规范实现，可按以下顺序扩展：

1. 把 `IllegalInstruction` 扩展成通用 CSR 仿真分发表；
2. 为 `LoadGuestPageFault` 增加 stage-2 缺页补齐与重试机制；
3. 引入更完整的 trap 注入 guest 能力（而不是统一 panic）。

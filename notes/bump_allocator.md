# bump_allocator 练习笔记

## 1) 练习目标

实现 `arceos/modules/bump_allocator/src/lib.rs` 里的 `EarlyAllocator`，用于 `alt_alloc` 练习。

这个分配器的定位是“早期分配器”：

- 字节分配从低地址向高地址增长（`b_pos` 前进）
- 页分配从高地址向低地址增长（`p_pos` 后退）
- 两端相向而行，满足 `b_pos <= p_pos`


## 2) 我的思考过程

### 2.1 初始想法

我一开始的想法是：

- 初始化后先把区域当作页区；
- 需要字节分配时，再“切一页”给字节区；
- 字节指针和页指针在边界处动态扩张。

后来对照注释和现有实现思路，发现更直接的模型是：

- `init` 后直接设 `b_pos = start`，`p_pos = end`；
- 中间整块都是可用区；
- 字节和页分别从两端申请，而不是“页区再切字节区”。


### 2.2 add_memory 的困惑

我最困惑的是 `add_memory`：

- 如果 allocator 已经在运行，后续扩容会不会破坏当前状态？

结论是：

- `add_memory` 在通用分配器里用于追加可用内存；
- 但当前这个 EarlyAllocator 是“单连续区 + 双端指针”模型，不适合无脑接收离散内存段；
- 所以本次实现采用保守策略：只支持“和现有区间连续拼接且不破坏当前状态”的情况，其余返回错误。


## 3) 先读项目里已有 allocator 的收获

重点阅读了 `axalloc`：`arceos/modules/axalloc/src/lib.rs`。

- `axalloc` 是两级分配：字节分配器 + 页分配器；
- 字节分配失败时，向页分配器要新内存扩展；
- 运行时初始化会先 `global_init` 最大 free 区，再 `global_add_memory` 其他 free 区。

这让我明确了两个点：

1. `add_memory` 在接口层面是有意义的；
2. EarlyAllocator 可以先做“简化但正确”的支持策略，不必一口气做到多区间复杂管理。


## 4) 具体实现过程

最终在 `arceos/modules/bump_allocator/src/lib.rs` 做了以下实现。

### 4.1 状态设计

维护字段：

- `start` / `end`：管理区间边界；
- `b_pos`：字节端当前指针；
- `p_pos`：页端当前指针；
- `b_count`：字节分配“活跃分配次数”；
- `p_count`：页分配累计页数（页端不回收）。


### 4.2 BaseAllocator

#### init

- 校验 `SIZE` 为 2 的幂；
- 校验 `size > 0`；
- 用 `checked_add` 计算 `end`，避免溢出；
- 初始化 `start/end/b_pos/p_pos` 并清零计数。

#### add_memory

采用“受限扩容”：

- 允许未初始化时直接等价 `init`；
- 允许向低端拼接（`end == self.start`），但要求字节端当前空闲（`b_count == 0`）；
- 允许向高端拼接（`start == self.end`），但要求页端未使用（`p_count == 0`）；
- 重叠返回 `MemoryOverlap`；
- 其他不支持场景返回 `NoMemory`。


### 4.3 ByteAllocator

#### alloc(layout)

- 对 `b_pos` 做对齐上取整（`align_up`）；
- 计算 `alloc_end = alloc_start + layout.size()`（用 `checked_add`）；
- 若 `alloc_end <= p_pos`，分配成功并更新 `b_pos`、`b_count += 1`；
- 否则返回 `AllocError::NoMemory`。

#### dealloc

- 仅做计数回收：`b_count -= 1`；
- 当 `b_count == 0` 时，按注释语义把字节区整体回收：`b_pos = start`。

#### 统计

- `used_bytes`：有活跃分配时返回 `b_pos - start`，否则为 0；
- `available_bytes`：`p_pos - b_pos`；
- `total_bytes`：`p_pos - start`。


### 4.4 PageAllocator

#### alloc_pages(num_pages, align_pow2)

- 校验 `num_pages > 0`、`align_pow2 > 0`；
- `align = max(align_pow2, PAGE_SIZE)`，并校验是 2 的幂；
- 计算 `alloc_size = num_pages * PAGE_SIZE`（`checked_mul`）；
- 从 `p_pos` 向下扣减，再按 `align` 向下对齐；
- 若新起点仍不越过 `b_pos`，则成功并更新 `p_pos`、`p_count`。

#### dealloc_pages

- 按 early allocator 约束，不回收页，保持 no-op。

#### 统计

- `used_pages = p_count`；
- `available_pages = (p_pos - b_pos) / PAGE_SIZE`；
- `total_pages = used_pages + available_pages`。


## 5) 踩坑记录

实现中踩过的典型坑：

- 字段名写混（`count` vs `b_count`）导致编译失败；
- `NonNull::new_unchecked` 忘了 `unsafe`；
- `b_pos` 清零成 `0` 是错误的，应回到 `start`；
- 对齐/乘法/加减法没有溢出保护；
- 页大小常量引用写成 `PAGE_SIZE` 而不是 `Self::PAGE_SIZE`。


## 6) 验证结果

已完成以下验证：

- `cargo check -p bump_allocator` 通过；
- `./scripts/test-alt_alloc.sh` 通过；
- 关键输出包含：`Bump tests run OK!`、`alt_alloc pass`。


## 7) 当前实现的边界与后续改进

当前 `add_memory` 是“保守可用”版本，重点是保证不破坏双端模型。

如果后续要支持真正的多段内存追加，需要引入额外元数据（例如区间表/空闲结构），复杂度会明显上升。

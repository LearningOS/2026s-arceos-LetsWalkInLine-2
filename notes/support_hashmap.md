# support_hashmap笔记


## 1) HashMap 的工作流水线（从 key 到桶位）

可以把一次 `insert/get` 想成 4 个阶段：

1. **序列化 key 到字节流**
   - Rust 里是通过 `Hash` trait 把 key 喂给 `Hasher::write(...)`。

2. **得到哈希值（u64）**
   - `Hasher::finish()` 返回一个 `u64`。

3. **把哈希值映射到表位置**
   - 常见做法是 `index = hash & (capacity - 1)`（capacity 是 2 的幂时）。

4. **冲突处理 + 探测**
   - 如果目标位置已被占用，需要按策略继续探测（开放寻址）或走链表（链地址法）。

`HashMap` 的性能核心就在第 4 步：碰撞越集中、探测链越长，性能越差。

## 2) 为什么需要“随机种子状态”

### 2.1 不是“为了随机而随机”

随机种子的主要目的：**让哈希函数实例化后不可预测**，降低构造性碰撞（HashDoS）风险。

- 如果哈希算法和种子固定，攻击者可以离线构造一批 key，让它们落到同一批桶；
- 随机种子让每次进程/每个 map 的哈希空间都不同，攻击者很难提前命中碰撞结构。

### 2.2 这次实现里怎么做的

在 `RandomState::new()` 里调用：

```rust
let seed = arceos_api::modules::axhal::misc::random();
```

然后拆成 `seed_lo` + `seed_hi`（两个 `u64`），交给 `DefaultHasher::with_keys(...)`。

直观理解：**同样的 key，在不同 `HashMap` 实例里，最终 hash 不再固定一致**。

### 2.3 为什么“没有真随机”也能工作

先区分两个概念：

- **功能正确性**：`HashMap` 只需要一个可用的哈希函数（`Eq + Hash` 契约成立），不要求密码学随机。
- **抗构造碰撞能力**：才需要“不可预测 seed”。

所以“没有硬件真随机”并不会让 `HashMap` 失效，只是可能降低抗 HashDoS 的强度。

换句话说：

- 没有真随机 -> 仍可正常 `insert/get/remove`；
- 只是 seed 的不可预测性变弱，极端输入下更容易被构造碰撞打性能。

## 3) `BuildHasher` 和 `Hasher` 为什么分成两层

### 3.1 `Hasher`

- 负责“吸收字节并输出 hash 值”；
- 生命周期通常是“一次哈希计算一个对象”。

### 3.2 `BuildHasher`

- 负责“创建 `Hasher`”；
- 通常持有全局配置（尤其是 seed/key）。

### 3.3 为什么不直接把 `Hasher` 塞进 `HashMap`

因为 map 内部会频繁做哈希计算，通常需要不断创建/复用“计算上下文”；
而 `BuildHasher` 让“算法配置（seed）”和“一次计算实例（Hasher）”解耦。

你可以把它理解成：

- `BuildHasher` = 配方模板（含盐值）；
- `Hasher` = 按模板开出来的一次性计算器。

## 4) 这次代码里的哈希器：做了什么，没做什么

实现文件：`arceos/ulib/axstd/src/collections/mod.rs`

### 4.1 做了什么

- `RandomState` 存随机种子；
- 实现 `BuildHasher`，把种子传给 `DefaultHasher`；
- `DefaultHasher` 用 FNV 风格混合 + key 混入；
- `HashMap` 默认参数设为 `S = RandomState`。

### 4.2 没做什么（有意取舍）

- 不是密码学哈希；
- 不是追求最强抗攻击设计；
- 目标是“学习友好 + 可运行 + 能体现 seed 机制”。

## 5) `hashbrown` 在这里扮演什么角色

## 5.1 它到底做了什么

`hashbrown` 负责的是 **哈希表结构本体**，而不是“哈希算法本身”。

它实现了现代高性能哈希表（SwissTable 思路）：

- 开放寻址（不走每桶链表）；
- 控制字节（control bytes）记录槽位状态；
- 分组探测（group probing），可利用 SIMD 思路快速筛选候选槽位；
- 平均情况下查找/插入常数开销很低。

所以分工是：

- `hashbrown::HashMap`：管理桶、探测、扩容、删除标记等结构逻辑；
- 你的 `BuildHasher/Hasher`：决定“同一个 key 映射到哪个哈希分布”。

### 5.2 `hashbrown` 在不知道 `axhal` 的情况下怎么“随机化”

`hashbrown` 本身不依赖你的 HAL。它把“默认 hasher”委托给 `foldhash::fast::RandomState`，再由 `foldhash` 做跨平台 seed 生成。

`foldhash` 的思路是 **best-effort nondeterminism（尽力提供非确定性）**，不是“必须拿到真随机设备”：

1. **每个 hasher seed（per-hasher seed）**
   - 混入栈地址（stack pointer）；
   - 在 `std` 下会混入线程本地状态；
   - 在 `no_std` 下改用全局原子状态来引入扰动。

2. **共享全局 seed（shared/global seed）**
   - 混入函数地址、静态对象地址等地址空间信息（利用 ASLR 提供变化）；
   - 若有 `std`，还会混入时间和堆对象地址等额外信息；
   - 在原子能力受限平台上可能退化为固定全局 seed。

3. **最终再做若干轮混合（mix）**
   - 把上述来源压缩成内部 seed，交给 hasher 使用。

结论：

- `hashbrown` 不需要知道 `axhal` 也能运行；
- 它靠通用信息源做 seed 初始化；
- 这能满足通用场景，但安全强度不等同于“来自硬件熵源的强随机”。

## 6) 不用 `hashbrown`，手写可不可以

可以，但工程量和坑都明显更大。你至少要自己解决：

1. 桶布局与内存管理（含扩容/缩容）；
2. 冲突策略（线性探测、二次探测、Robin Hood 等）；
3. 删除语义（tombstone）与再插入一致性；
4. 装载因子阈值和 rehash 策略；
5. 迭代器、借用规则、entry API 等接口细节；
6. 性能退化场景与边界条件测试。

## 7) 这次实现和 HashMap 机理的对应关系（对照看）

- **机理里的“随机化哈希空间”** -> `RandomState::new()` 从 `axhal::misc::random()` 取 seed。
- **机理里的“哈希器工厂”** -> `impl BuildHasher for RandomState`。
- **机理里的“具体哈希计算”** -> `impl Hasher for DefaultHasher` 的 `write/finish`。
- **机理里的“表结构与冲突处理”** -> 交给 `hashbrown::HashMap`。

这就是这份实现的核心逻辑分层。

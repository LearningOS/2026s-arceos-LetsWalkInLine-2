# ramfs_rename 练习笔记

## 1) 练习目标

在不修改 `exercises/` 代码的前提下，让 `fs::rename("/tmp/f1", "/tmp/f2")` 在 ramfs 上可用，并通过脚本：

- `./scripts/test-ramfs_rename.sh`

核心修改点限定为：

- `arceos/axfs_ramfs/src/dir.rs`
- `arceos/Cargo.toml` 的 patch 配置


## 2) 我的思考历程（从直觉到正确模型）

### 2.1 初始想法

最开始容易想到：

- 给 `DirNode` 和 `FileNode` 都加 `rename`。
- 目录重命名可能要“逐级向上改名”。

### 2.2 关键纠正

真正的模型是：

- `FileNode` 只保存内容，不保存名字。
- 名字存在父目录的 `children: BTreeMap<String, VfsNodeRef>` 里。
- 所以 rename 本质是“改目录项 key”，不改文件节点本体。

也就是：

- 只需要在目录侧实现 rename（`DirNode`）。
- 不是“向上改名”，而是按路径从当前目录向下递归找到目标目录，再做 key 变更。


## 3) 我踩过的错误（这题最容易错的地方）

### 3.1 错误一：以为改了 `axfs_ramfs` 就一定生效

症状：

- 运行时仍报 `AxError::Unsupported`（来自 `axfs_vfs` 默认 `rename`）。

根因：

- 实际构建没稳定使用本地 `axfs_ramfs`。
- 加了 `[patch.crates-io]` 后又遇到版本不匹配：本地 `axfs_ramfs/Cargo.toml` 是 `0.1.1`，锁里依赖是 `0.1.2`，导致 patch 未被使用。

修复：

1. 在 `arceos/Cargo.toml` 增加：
   - `axfs_ramfs = { path = "./axfs_ramfs" }` 到 `[patch.crates-io]`
2. 将 `arceos/axfs_ramfs/Cargo.toml` 版本对齐为 `0.1.2`
3. `cargo update -p axfs_ramfs` 让 lockfile 使用本地路径包


### 3.2 错误二：rename 逻辑只按“纯相对路径”处理

症状：

- `Unsupported` 消失后，变成 `Invalid input parameter`。

根因：

- 调用链里 `src_path` 可能已被去掉挂载点（如 `f1`），但 `dst_path` 仍可能是绝对路径（如 `/tmp/f2`）。
- 若直接要求同层名字严格对应，会提前判错。

修复：

- 在 `DirNode::rename` 中，当发现 `dst_path` 是绝对路径且路径层级不对齐时，递归剥离 `dst` 前缀，直到与 `src` 处于同一层再执行末级改名。
- 同时保留约束：仅在“绝对路径前缀残留”场景放行；其它不对齐仍返回 `InvalidInput`，避免意外支持 move。


## 4) 结构理解（这题真正要吃透的抽象）

以 `DirNode` 为例：

- `this: Weak<DirNode>`：自身弱引用，用于给新建子目录设置 parent。
- `parent: RwLock<Weak<dyn VfsNodeOps>>`：父节点引用。
- `children: RwLock<BTreeMap<String, VfsNodeRef>>`：当前目录直接子项（文件/目录都在这里）。

因此 rename 的本质操作是：

- `children.remove(src_name)`
- `children.insert(dst_name, node)`


## 5) 最终实现详解（`dir.rs` 的 rename）

实现位置：`arceos/axfs_ramfs/src/dir.rs`。

### 5.1 入口与路径拆分

- `split_path(src_path)` -> `(src_name, src_rest)`
- `split_path(dst_path)` -> `(dst_name, dst_rest)`

其中 `split_path` 会先 `trim_start_matches('/')`，再按第一个 `/` 切分。


### 5.2 分支 A：`(Some, Some)`，两边都还有后续路径

- `"" | "."`：当前层不推进目录，继续递归。
- `".."`：切换到父目录递归。
- 其他名字：
  - 若 `src_name == dst_name`，进入同名子目录继续递归。
  - 若不相等：
    - 若 `dst_path` 以 `/` 开头，说明可能还带挂载点前缀，递归剥离 `dst_rest`。
    - 否则 `InvalidInput`。


### 5.3 分支 B：`(None, Some)`，src 到末级，dst 还没到

- 仅在 `dst_path` 为绝对路径时允许继续剥离前缀。
- 其它情况 `InvalidInput`。


### 5.4 分支 C：`(None, None)`，末级执行真正改名

1. 拒绝空名、`.`、`..`。
2. 写锁 `children`。
3. `remove(src_name)` 取出节点，不存在则 `NotFound`。
4. `insert(dst_name, node)` 完成改名。


### 5.5 分支 D：`(Some, None)`

- 源比目标更深，判为非法：`InvalidInput`。


## 6) 用例走读：`/tmp/f1 -> /tmp/f2`

在 root/mount 包装后，进入 ramfs 时常见是：

- `src_path = "f1"`
- `dst_path = "/tmp/f2"`

流程：

1. 命中 `(None, Some("f2"))`，识别 `dst` 是绝对路径，继续剥离。
2. 递归成 `rename("f1", "f2")`。
3. 命中 `(None, None)`，执行 `children` 内 key 改名。
4. 随后读取 `/tmp/f2` 成功，内容仍为 `hello`。


## 7) 本次实际改动文件

- `arceos/axfs_ramfs/src/dir.rs`：新增 `DirNode::rename` 实现。
- `arceos/Cargo.toml`：在 `[patch.crates-io]` 增加本地 `axfs_ramfs`。
- `arceos/axfs_ramfs/Cargo.toml`：版本改为 `0.1.2` 以匹配锁定版本。
- `arceos/Cargo.lock`：由 `cargo update -p axfs_ramfs` 自动更新为本地路径来源。


## 8) 验证结果（容器内）

执行：

- `./scripts/test-ramfs_rename.sh`

关键输出：

- `Read '/tmp/f2' content: [hello] ok!`
- `[Ramfs-Rename]: ok!`
- `ramfs_rename pass`


## 9) 这题的经验总结

1. 先确认“名字归属层”：名字属于目录项，不属于文件节点。
2. 先确认“依赖是否真生效”：patch + version + lock 三件套缺一不可。
3. 在挂载体系里调路径相关逻辑，必须考虑“src/dst 规范化不对称”的现实调用形态。

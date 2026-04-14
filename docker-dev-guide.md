# Docker / Docker Compose / Dev Container 使用说明

本文档说明如何在本项目中使用容器进行开发，包含三种方式：

- 仅使用 Docker（`docker run`）
- 使用 Docker Compose（推荐）
- 使用 VS Code Dev Container（推荐）

当前约定镜像名为 `acreos-guid:latest`。

## 0. 前置要求

- 已安装 Docker Desktop（或 Docker Engine + Compose Plugin）
- 本机可正常执行：`docker --version` 和 `docker compose version`
- 当前目录为仓库根目录

## 1. 使用 Dockerfile 构建镜像

在仓库根目录执行：

```bash
docker build -t acreos-guid:latest .
```

可选校验：

```bash
docker images acreos-guid
```

如果你更新了 `Dockerfile`，需要重新执行一次 `docker build`。

## 2. 仅使用 Docker 启动开发容器

这种方式不依赖 Compose，适合快速进入容器。

Linux/macOS:

```bash
docker run --rm -it \
  --name arceos-dev \
  -v "$(pwd):/mnt" \
  -w /mnt \
  acreos-guid:latest \
  bash
```

PowerShell:

```powershell
docker run --rm -it --name arceos-dev -v "${PWD}:/mnt" -w /mnt acreos-guid:latest bash
```

进入后即可在容器中开发，例如：

```bash
cd /mnt
./scripts/total-test.sh
```

## 3. 使用 Docker Compose（推荐）

项目已提供 `docker-compose.yml`，核心逻辑：

- 使用镜像 `acreos-guid:latest`
- 将仓库挂载到容器 `/mnt`
- 设置工作目录为 `/mnt`
- 通过命名卷缓存 Cargo 下载内容（`cargo-registry`、`cargo-git`）

### 3.1 启动

```bash
docker compose up -d
```

### 3.2 进入容器

```bash
docker compose exec dev bash
```

### 3.3 停止与清理

停止容器：

```bash
docker compose down
```

停止并清理缓存卷：

```bash
docker compose down -v
```

### 3.4 常用排查

查看容器状态：

```bash
docker compose ps
```

查看服务定义是否有效：

```bash
docker compose config
```

## 4. 使用 VS Code Dev Container（推荐）

项目已提供 `.devcontainer/devcontainer.json`，它会复用 `docker-compose.yml` 中的 `dev` 服务。

### 4.1 准备

- 安装 VS Code 扩展：`Dev Containers`
- 建议同时安装：`rust-analyzer`

### 4.2 打开项目

在 VS Code 打开仓库根目录后，执行：

- `Dev Containers: Reopen in Container`

首次进入时，VS Code 会自动创建/启动容器并附着到其中。

### 4.3 日常使用

- VS Code 终端默认就在容器里
- 工作目录是 `/mnt`
- 代码仍是你本机这份仓库（通过挂载实现）

## 5. 可选：Zed 的使用建议

Zed 当前更推荐通过 SSH 做远程开发，不是直接 Dev Container 流程。对本项目建议：

- 用 Zed 直接打开本机仓库目录进行编辑
- 用终端执行容器命令进行构建/测试：`docker compose exec dev bash`

如果你确实希望 Zed 直接连接容器，可额外配置容器内 `sshd`，再按 Zed 的 SSH Remote 流程连接。

## 6. 常见问题

1) `docker compose up -d` 报找不到镜像

- 先执行：`docker build -t acreos-guid:latest .`

2) 在 Windows 上卷挂载失败

- 检查 Docker Desktop 的文件共享设置
- 若使用 WSL，建议在 WSL 终端中运行 Docker 命令

3) Rust 依赖下载慢

- Compose 已启用 Cargo 缓存卷
- 如网络不稳定，可在容器内额外配置 cargo 镜像源

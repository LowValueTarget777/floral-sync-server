# 发布管理

这份文档面向维护者，描述 Floral Sync Server 的版本管理、二进制发布、Docker 镜像发布和发布后核对流程。

## 1. 版本号

项目版本号定义在仓库根目录的 `Cargo.toml`：

- 二进制发布使用这里的版本号
- Docker 镜像默认也使用这里的版本号作为 tag
- 发布时通常同时更新一个 `latest` 标签

推荐使用语义化版本号，例如：

- `0.1.1`
- `0.2.0`
- `1.0.0`

## 2. 发布前检查

发布前至少执行：

```bash
npm --prefix admin-ui ci
npm --prefix admin-ui test
cargo test
cargo build --release
```

如果发布内容涉及部署、配置、镜像行为或协议变更，请同步检查：

- `README.md`
- `docs/deployment.md`
- `docs/build.md`
- `docs/protocol.md`

## 3. 二进制发布

### Windows 主机

```powershell
./scripts/build.ps1
```

或直接使用底层发布脚本：

```powershell
./scripts/release.ps1
```

### Linux / macOS 主机

```bash
bash ./scripts/build.sh
```

或直接使用底层发布脚本：

```bash
bash ./scripts/release.sh
```

最终二进制会汇总到 `target/release-artifacts/`。

## 4. Docker 镜像发布

公开镜像仓库：`namelsscinder/floral-sync-server`

### 仅发布 amd64

Windows:

```powershell
./scripts/docker-release.ps1 -Image namelsscinder/floral-sync-server -Platform linux/amd64 -Push
```

Linux / macOS:

```bash
bash ./scripts/docker-release.sh --image namelsscinder/floral-sync-server --platform linux/amd64 --push
```

### 发布 amd64 + arm64

Windows:

```powershell
./scripts/docker-release.ps1 -Image namelsscinder/floral-sync-server -Platform linux/amd64,linux/arm64 -Push
```

Linux / macOS:

```bash
bash ./scripts/docker-release.sh --image namelsscinder/floral-sync-server --platform linux/amd64 --platform linux/arm64 --push
```

默认行为：

- 从 `Cargo.toml` 读取版本号作为镜像 tag
- 同时发布 `latest`
- 不带 `-Push` / `--push` 时仅在本地构建并加载镜像

## 5. 发布后核对

镜像发布后，检查远端标签是否可见：

```powershell
docker buildx imagetools inspect namelsscinder/floral-sync-server:0.1.1
docker buildx imagetools inspect namelsscinder/floral-sync-server:latest
```

你应确认：

- 版本标签存在
- `latest` 指向刚发布的版本
- 平台列表符合预期，例如 `linux/amd64` 或 `linux/amd64` + `linux/arm64`

## 6. 发布记录建议

每次对外发布后，建议至少完成：

- 创建对应的 git tag
- 在代码托管平台编写 release notes
- 记录本次发布包含的部署或兼容性变更

如果这次发布修复了部署或运维问题，优先在 `README.md` 和 `docs/deployment.md` 中反映最终用户可见的行为变化。
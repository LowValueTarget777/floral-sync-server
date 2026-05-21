# 构建与发布文档

这份文档描述 Floral Sync Server 的本地构建、发布构建和交叉编译流程。

## 1. 依赖要求

### 所有构建都需要

- Rust stable toolchain
- Node.js 20+ 和 npm

### 仅交叉编译 Linux 产物需要

- Zig
- `cargo-zigbuild`
- `rustup target add x86_64-unknown-linux-gnu x86_64-unknown-linux-musl`

这里的 `unknown` 是 Rust target triple 里的 vendor 占位符，对 `cargo` / `rustup` 的目标名有意义；
但对最终用户看到的发布文件名没有额外信息量，所以仓库里的 Linux 产物文件名会简化为
`x86_64-linux-gnu` 和 `x86_64-linux-musl`。

## 2. 本地开发构建

安装前端依赖：

```bash
npm --prefix admin-ui ci
```

执行调试构建：

```bash
cargo build
```

执行发布构建：

```bash
cargo build --release
```

构建输出：

- 调试构建：`target/debug/floral-sync-server`
- 发布构建：`target/release/floral-sync-server`
- Windows 发布构建：`target/release/floral-sync-server.exe`

## 3. 为什么 Rust 构建前需要 Node.js

仓库根目录的 `build.rs` 会在 `cargo build` 和 `cargo test` 阶段自动执行：

```text
npm --prefix admin-ui run build
```

这一步会把 `admin-ui/` 里的 React 管理后台构建成静态资源，然后嵌入 Rust 二进制。
因此即使你只改 Rust 代码，只要仓库里启用了管理后台构建，也必须保证本地 Node.js 和 npm 可用。

## 4. 构建不带管理后台的 lite 二进制

如果你只需要同步服务，不需要内嵌管理后台，可以直接关闭默认 feature：

```bash
cargo build --release --no-default-features
```

这会生成一个 sync-only 变体：

- 不再执行 `admin-ui` 的前端构建
- 不需要 Node.js / npm
- 不包含管理后台静态资源和管理 API 路由

构建输出路径仍然是：

- `target/release/floral-sync-server`
- Windows 下对应 `target/release/floral-sync-server.exe`

## 5. 验证构建产物

推荐在构建完成后做一次最小验证：

```bash
./target/release/floral-sync-server config show
```

如果同目录没有 `sync-server.toml`，命令会自动生成默认配置文件，这也能同时验证二进制是否能正常启动配置流程。

## 6. 便捷构建脚本

优先使用仓库里的便捷入口脚本：

- Windows 主机：`./scripts/build.ps1`
- Linux / macOS 主机：`bash ./scripts/build.sh`

它们会把目标选择转换成底层 `release` 脚本需要的参数，并继续把产物汇总到 `target/release-artifacts/`。

如果你希望在同一轮发布构建里额外产出 lite 变体：

- Windows 主机：`./scripts/build.ps1 -IncludeLite`
- Linux / macOS 主机：`bash ./scripts/build.sh --include-lite`

### Windows

默认同时构建：

- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`

执行：

```powershell
./scripts/build.ps1
```

仅构建 Windows：

```powershell
./scripts/build.ps1 -Target windows
```

仅构建 Linux：

```powershell
./scripts/build.ps1 -Target linux
```

可选 dry-run：

```powershell
./scripts/build.ps1 -DryRun
```

### Linux / macOS

默认同时构建：

- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`

执行：

```bash
bash ./scripts/build.sh
```

仅构建 musl：

```bash
bash ./scripts/build.sh --target linux-musl
```

可选 dry-run：

```bash
bash ./scripts/build.sh --dry-run
```

## 6. 平台发布脚本

### Windows

在 Windows 主机执行：

```powershell
./scripts/release.ps1
```

这是底层发布脚本，`scripts/build.ps1` 会调用它。脚本会：

- 安装 `admin-ui` 依赖
- 产出 Windows `x86_64-pc-windows-msvc` 二进制
- 用 `cargo-zigbuild` 交叉编译 Linux GNU / musl 二进制
- 把产物汇总到 `target/release-artifacts/`

可选 dry-run：

```powershell
./scripts/release.ps1 -DryRun
```

### Linux / macOS

在 Linux 或 macOS 主机执行：

```bash
bash ./scripts/release.sh
```

这是底层发布脚本，`scripts/build.sh` 会调用它。脚本会：

- 安装 `admin-ui` 依赖
- 构建 Linux GNU 二进制
- 构建 Linux musl 二进制
- 把产物汇总到 `target/release-artifacts/`

可选 dry-run：

```bash
bash ./scripts/release.sh --dry-run
```

## 7. 常见构建失败原因

### `npm` 不存在

安装 Node.js，并确认 `npm` 或 `npm.cmd` 在 PATH 中可用。

### `cargo build` 期间前端打包失败

先手动执行：

```bash
npm --prefix admin-ui ci
npm --prefix admin-ui run build
```

单独查看 Vite / TypeScript 错误，再回到 Rust 构建。

### `cargo zigbuild` 不存在

安装：

```bash
cargo install cargo-zigbuild --locked
```

### 缺少 Zig

先安装 Zig，再重新执行 `scripts/release.ps1` 或 `scripts/release.sh`。

## 8. 发布前检查清单

建议至少完成：

```bash
npm --prefix admin-ui test
cargo test
cargo build --release
```

如果你要对外发布二进制，还应确认：

- `docs/deployment.md` 与当前配置行为一致
- `README.md` 中的快速开始命令仍然可执行
- 同步协议没有破坏 `docs/protocol.md` 中约定的路径与字段
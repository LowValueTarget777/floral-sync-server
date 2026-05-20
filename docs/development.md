# 开发文档

这份文档面向日常维护 Floral Sync Server 的开发者，覆盖本地环境、常用命令、联调方式和提交流程。

## 1. 环境要求

基础开发环境：

- Rust stable toolchain
- Node.js 20+ 和 npm
- Git

仅在需要产出 Linux GNU / musl 发布文件时额外需要：

- Zig
- `cargo-zigbuild`

## 2. 首次拉起开发环境

在仓库根目录执行：

```bash
npm --prefix admin-ui ci
npm --prefix admin-ui test
cargo test
```

这里的顺序有意先验证前端依赖，再跑整个 Rust 测试套件。`cargo test` 和 `cargo build`
期间会自动触发 `admin-ui` 的生产构建，并把静态资源嵌入 Rust 二进制。

## 3. 本地启动

先生成默认配置：

```bash
cargo run -- config show
```

第一次执行会在仓库根目录生成 `sync-server.toml`，并按默认配置创建随机同步 Token。

随后启动服务：

```bash
cargo run
```

默认访问地址：

- 同步接口：`http://127.0.0.1:8787`
- 管理后台：`http://127.0.0.1:8788/admin`

第一次打开管理后台时，如果还没有管理员密码，会进入一次性引导设置流程。

## 4. 日常开发命令

### 后端测试

```bash
cargo test
```

### 前端测试

```bash
npm --prefix admin-ui test
```

### 发布构建验证

```bash
cargo build --release
./target/release/floral-sync-server config show
```

Windows 下最后一条命令对应 `./target/release/floral-sync-server.exe config show`。

### 恢复同步验证

如果你修改了备份 / 恢复或同步 revision 语义，优先跑仓库里的真实运行态验证脚本：

```powershell
./scripts/verify-restore.ps1
```

这个脚本会：

- 在 `target/` 下创建一个临时独立运行目录
- 启动一个真实服务端实例
- 通过同步 API 和管理 API 复现“删除后恢复”和“备份外新增笔记”两个场景
- 断言旧 `since` 游标能收到恢复出来的笔记，以及备份外新增笔记会收到 tombstone

默认成功后会自动清理临时目录；如果你需要保留日志、配置和 SQLite 文件做排查，可以执行：

```powershell
./scripts/verify-restore.ps1 -KeepArtifacts
```

## 5. 代码结构

- `src/`: Rust 服务端代码，包括同步 API、管理 API、配置、鉴权与存储。
- `admin-ui/`: React + Vite 管理后台源码与测试。
- `docs/`: 协议、部署、开发、构建和迁移文档。
- `scripts/`: 发布脚本与针对关键运维路径的手工验证脚本。
- `build.rs`: 构建时触发 `admin-ui` 生产打包，并把静态资源嵌入二进制。

## 6. 前后端协作方式

这个仓库的默认工作流是“由 Rust 二进制托管嵌入后的管理后台”，因此最可靠的联调方式是：

1. 修改 `admin-ui/` 或 `src/`。
2. 运行 `cargo run`。
3. 通过嵌入后的后台页面验证变更。

如果只做前端组件或页面结构调整，也可以单独运行：

```bash
npm --prefix admin-ui run dev
```

但当前仓库没有为 Vite 开发服务器配置后端代理，因此这个模式更适合纯界面迭代和单元测试；涉及真实登录、设置保存、备份等 API 行为时，仍建议通过 `cargo run` 做完整验证。

## 7. 会生成哪些本地文件

以下内容属于开发或运行期产物，不应该提交到仓库：

- `target/`
- `admin-ui/node_modules/`
- `admin-ui/dist/`
- `floral/`
- `.env`
- `.env.*`
- `docker/.env`
- `sync-server.toml`
- `data/`
- `logs/`
- `exports/`
- `*.sqlite3`、`*.sqlite3-shm`、`*.sqlite3-wal`

这些路径已经写入仓库根目录的 `.gitignore`。

## 8. 提交改动前建议检查

最小建议检查：

```bash
npm --prefix admin-ui test
cargo test
cargo build --release
```

如果改动涉及接口、配置或部署行为，请再检查：

- `docs/protocol.md`
- `docs/deployment.md`
- `README.md`

确保文档与当前实现一致。
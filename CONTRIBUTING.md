# Contributing

欢迎为 Floral Sync Server 提交问题、文档修订和代码改进。

## 开始之前

- 先阅读 `README.md` 了解仓库边界和运行方式。
- 开发环境、日常命令和联调说明见 `docs/development.md`。
- 构建、发布和交叉编译说明见 `docs/build.md`。
- 对外版本发布和 Docker 镜像发布流程见 `docs/release.md`。
- 同步协议是稳定接口，涉及 `/health`、`/v1/changes`、`/v1/push` 的修改前请先检查 `docs/protocol.md`。

## 提交改动前的最小检查

在仓库根目录执行：

```bash
npm --prefix admin-ui ci
npm --prefix admin-ui test
cargo test
cargo build --release
```

如果你改动了部署、配置、协议或发布流程，请同步更新 `docs/` 里的相应文档。

## 贡献约定

- 保持同步 API 向后兼容，避免破坏 Floral Notepaper 客户端。
- 优先做小而明确的改动，避免把文档、重构和功能变更混在一个提交里。
- 新增配置项时，同时补充默认值、迁移行为和部署说明。
- 管理后台的写操作必须说明安全边界；只读能力优先保持简单。
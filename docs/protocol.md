# 同步协议

这个协议刻意保持精简。客户端保存本地 Markdown 文件和本地
`sync_state.json`，服务端保存每篇笔记的最新版本以及单调递增的 revision。

## 鉴权

每个请求都必须带 Bearer Token：

```text
Authorization: Bearer <token>
```

服务端会把这个 Token 与当前运行配置里的 `sync_token` 比较。当前版本没有用户体系、
会话、刷新令牌，也没有项目或空间隔离。

## 数据模型

每条笔记变更使用以下结构：

```json
{
  "id": "note-id",
  "title": "Title",
  "content": "Markdown body",
  "category": "Inbox",
  "createdAt": "2026-05-18T08:00:00Z",
  "updatedAt": "2026-05-18T08:10:00Z",
  "deletedAt": null,
  "contentHash": "stable-client-hash",
  "deviceId": "client-device-id"
}
```

删除操作会设置 `deletedAt`，此时 `title`、`content` 和 `category`
可以为空。服务端仍然会保留墓碑，避免旧客户端用过期的本地状态把笔记重新建回来。

## `GET /health`

返回当前服务端 revision：

```json
{
  "ok": true,
  "revision": 42
}
```

客户端会用这个接口同时检查服务可达性和 Token 是否有效。

## `GET /v1/changes?since=<revision>`

返回所有服务端 revision 大于 `since` 的最新笔记记录：

```json
{
  "revision": 42,
  "changes": [
    {
      "revision": 42,
      "note": {
        "id": "note-id",
        "title": "Title",
        "content": "Markdown body",
        "category": "Inbox",
        "createdAt": "2026-05-18T08:00:00Z",
        "updatedAt": "2026-05-18T08:10:00Z",
        "deletedAt": null,
        "contentHash": "stable-client-hash",
        "deviceId": "client-device-id"
      }
    }
  ]
}
```

服务端保存的是“最新状态”，不是完整历史日志。如果客户端拉取之前同一篇笔记
已经在服务端更新了多次，客户端拿到的是最新状态和对应 revision。

管理端如果执行“恢复备份”，服务端会把恢复出来的当前状态重新映射到新的、
更高的 revision，并为备份里缺失但恢复前存在的笔记补 tombstone。这样已经持有
更高 `since` 游标的客户端仍然能收敛到恢复后的服务端状态，而不需要先手动清空
本地 `sync_state.json`。

## `POST /v1/push`

客户端在应用完远端更新后，再把本地变更推送上来：

```json
{
  "deviceId": "client-device-id",
  "changes": []
}
```

服务端通过比较 `deletedAt || updatedAt` 来执行最后写入胜出。更旧的传入
变更会被忽略，被接受的变更会拿到新的服务端 revision。

响应：

```json
{
  "revision": 43
}
```

## 冲突策略

面向用户的冲突备份由客户端负责处理，服务端只保留最终胜出的最新状态。这样
服务端能保持轻量，同时桌面端可以把冲突落败的本地版本保存成普通 Markdown
笔记，方便用户后续手动整理。

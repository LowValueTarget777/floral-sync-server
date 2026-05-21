# 部署说明

## 构建

```bash
npm --prefix admin-ui ci
cargo build --release
```

生成的发布二进制文件位于 `target/release/floral-sync-server`。

如果你需要一次性产出 Windows、Linux GNU 和 Linux musl 的发布文件：

Windows:

```powershell
./scripts/release.ps1
```

Linux / macOS:

```bash
bash ./scripts/release.sh
```

脚本会把最终产物汇总到 `target/release-artifacts/`。

## Docker 部署

如果你想把 Docker 相关内容和项目其他文件隔离开，仓库已经把 Docker 资产全部放在
`docker/` 目录里：

- `docker/Dockerfile`
- `docker/compose.yml`
- `docker/compose.build.yml`
- `docker/entrypoint.sh`
- `docker/healthcheck.sh`
- `docker/.env.example`

这个部署方式默认使用宿主机 bind mount 保存配置、数据库、备份导出和日志，所以运行期文件
会落到你指定的本地目录里，而不是藏在匿名卷或容器内部。

执行下面的命令前，请先确保本机 Docker Engine / Docker Desktop 已经启动。

### 1. 可选环境变量覆盖

如果你想固定镜像地址、端口、Token 或宿主机目录，可以先复制模板：

```powershell
Copy-Item docker/.env.example docker/.env
```

如果你不复制 `.env`，容器首次启动时也能直接工作：

- 会自动生成 `sync_token`
- 会自动生成 `admin_session_secret`
- 会把这两个值写入宿主机上的 `sync-server.toml`
- 会在启动日志里直接打印同步 Token

如果你想自己固定这些值，才需要填写：

- `FLORAL_SYNC_TOKEN`
- `FLORAL_ADMIN_SESSION_SECRET`

默认模板里的 `FLORAL_IMAGE` 已经指向公开镜像：

- `namelsscinder/floral-sync-server:latest`

如果你想固定到某个已验证版本，推荐改成明确版本号，例如：

- `namelsscinder/floral-sync-server:0.1.3`

`FLORAL_ADMIN_PASSWORD_HASH` 可以先留空。这样第一次打开管理后台时，会进入一次性引导流程
来设置管理员密码。

### 2. 选择宿主机持久化目录

`docker/.env` 里这四个变量控制宿主机绑定目录：

- `FLORAL_HOST_CONFIG_DIR`
- `FLORAL_HOST_DATA_DIR`
- `FLORAL_HOST_EXPORT_DIR`
- `FLORAL_HOST_LOG_DIR`

默认值是相对路径：

```dotenv
FLORAL_HOST_CONFIG_DIR=./floral/config
FLORAL_HOST_DATA_DIR=./floral/data
FLORAL_HOST_EXPORT_DIR=./floral/exports
FLORAL_HOST_LOG_DIR=./floral/logs
```

它们会解析到 `docker/compose.yml` 所在目录旁边。你也可以改成绝对路径。

Linux 示例：

```dotenv
FLORAL_HOST_CONFIG_DIR=/srv/floral-sync/config
FLORAL_HOST_DATA_DIR=/srv/floral-sync/data
FLORAL_HOST_EXPORT_DIR=/srv/floral-sync/exports
FLORAL_HOST_LOG_DIR=/srv/floral-sync/logs
```

Windows 示例：

```dotenv
FLORAL_HOST_CONFIG_DIR=D:/floral-sync/config
FLORAL_HOST_DATA_DIR=D:/floral-sync/data
FLORAL_HOST_EXPORT_DIR=D:/floral-sync/exports
FLORAL_HOST_LOG_DIR=D:/floral-sync/logs
```

首次启动时，容器会在 `FLORAL_HOST_CONFIG_DIR` 里生成 `sync-server.toml`。后续重启会直接复用这个
配置文件，不会覆盖你手工修改的内容。

如果你明确想让容器按当前环境变量重写配置文件，可以把 `FLORAL_FORCE_WRITE_CONFIG=1`。

### 3. 启动容器

#### 生产环境：直接拉已发布镜像

最推荐给最终用户的方式，是直接写一个最小 `compose.yml`：

```yaml
services:
  floral-sync-server:
    image: namelsscinder/floral-sync-server:0.1.3
    restart: unless-stopped
    ports:
      - "8787:8787"
      - "8788:8788"
    environment:
      FLORAL_SYNC_LISTEN: 0.0.0.0:8787
      FLORAL_ADMIN_LISTEN: 0.0.0.0:8788
      FLORAL_CONFIG_PATH: /var/lib/floral-sync/config/sync-server.toml
      FLORAL_DB_PATH: /var/lib/floral-sync/data/floral-sync.sqlite3
      FLORAL_EXPORT_DIR: /var/lib/floral-sync/exports
      FLORAL_LOG_PATH: /var/lib/floral-sync/logs/floral-sync-server.log
      FLORAL_LOG_LEVEL: info
    volumes:
      - ./floral/config:/var/lib/floral-sync/config
      - ./floral/data:/var/lib/floral-sync/data
      - ./floral/exports:/var/lib/floral-sync/exports
      - ./floral/logs:/var/lib/floral-sync/logs
    read_only: true
    tmpfs:
      - /tmp
      - /run
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
```

启动：

```bash
docker compose up -d
```

这种方式不依赖本地源码目录。只要目标机器上有这份 `compose.yml`，用户就不需要先克隆仓库。

如果你不想用 compose，等价的 `docker run` 示例是：

```bash
docker run -d \
  --name floral-sync-server \
  --restart unless-stopped \
  -p 8787:8787 \
  -p 8788:8788 \
  -e FLORAL_SYNC_LISTEN=0.0.0.0:8787 \
  -e FLORAL_ADMIN_LISTEN=0.0.0.0:8788 \
  -e FLORAL_CONFIG_PATH=/var/lib/floral-sync/config/sync-server.toml \
  -e FLORAL_DB_PATH=/var/lib/floral-sync/data/floral-sync.sqlite3 \
  -e FLORAL_EXPORT_DIR=/var/lib/floral-sync/exports \
  -e FLORAL_LOG_PATH=/var/lib/floral-sync/logs/floral-sync-server.log \
  -e FLORAL_LOG_LEVEL=info \
  -v "$(pwd)/floral/config:/var/lib/floral-sync/config" \
  -v "$(pwd)/floral/data:/var/lib/floral-sync/data" \
  -v "$(pwd)/floral/exports:/var/lib/floral-sync/exports" \
  -v "$(pwd)/floral/logs:/var/lib/floral-sync/logs" \
  --read-only \
  --tmpfs /tmp \
  --tmpfs /run \
  --security-opt no-new-privileges:true \
  --cap-drop ALL \
  namelsscinder/floral-sync-server:0.1.3
```

这里的版本号推荐固定为已验证版本。当前推荐固定到 `0.1.3`；如果你更看重快速跟进最新发布，也可以直接使用 `latest`。

#### 使用仓库内维护的 compose 模板

如果你想继续使用仓库里的 `docker/compose.yml` 和 `.env` 模板，也可以：

```powershell
$env:FLORAL_IMAGE='namelsscinder/floral-sync-server:0.1.3'
docker compose -f docker/compose.yml up -d
```

Linux / macOS:

```bash
FLORAL_IMAGE=namelsscinder/floral-sync-server:0.1.3 docker compose -f docker/compose.yml up -d
```

#### 仓库内本地开发：从源码构建镜像

如果你就在当前仓库里改代码，希望直接从源码构建并启动：

```powershell
docker compose -f docker/compose.yml -f docker/compose.build.yml up -d --build
```

```bash
docker compose -f docker/compose.yml -f docker/compose.build.yml up -d --build
```

如果你用了 `.env`，再按自己的需要加上 `--env-file docker/.env` 即可。

默认端口：

- 同步接口：`127.0.0.1:8787`
- 管理后台：`http://127.0.0.1:8788/admin`

`docker/.env` 里也可以覆盖：

- `FLORAL_SYNC_PUBLISHED_PORT`
- `FLORAL_ADMIN_PUBLISHED_PORT`
- `FLORAL_LOG_LEVEL`

### 4. 停止与清理

停止容器但保留宿主机目录里的数据：

```powershell
docker compose -f docker/compose.yml down
```

如果你要清掉 compose 创建的容器和网络：

```powershell
docker compose -f docker/compose.yml down -v
```

这不会删除你通过 bind mount 指定的宿主机目录。要彻底清空数据，请手工删除
`FLORAL_HOST_CONFIG_DIR`、`FLORAL_HOST_DATA_DIR`、`FLORAL_HOST_EXPORT_DIR` 和
`FLORAL_HOST_LOG_DIR` 对应的目录。

如果你是用 `docker/compose.build.yml` 启动的源码构建模式，停止时也建议带上同样的 compose 组合：

```powershell
docker compose -f docker/compose.yml -f docker/compose.build.yml down
```

如果你使用了 `.env` 覆盖项，再补上 `--env-file docker/.env` 即可。

## 启动后的管理员操作

服务启动后，终端或容器日志会直接打印当前同步 Token。这样你可以先把客户端接到服务端，再决定
要不要进入管理后台。

管理后台设置页支持：

- 显示 / 隐藏当前同步 Token
- 一键复制当前同步 Token
- 轮换同步 Token
- 一键请求服务重启

其中“重启服务”适合 Docker、systemd、Windows Service 这类有托管器的场景。按钮会让当前进程
优雅退出，再由外层托管器自动拉起新实例。

## Docker 镜像发布

如果你是维护者，希望每次改完代码都方便地重新生成并发布镜像，可以直接用仓库里的脚本。

Windows:

```powershell
./scripts/docker-release.ps1 -Image namelsscinder/floral-sync-server -Push
```

Linux / macOS:

```bash
bash ./scripts/docker-release.sh --image namelsscinder/floral-sync-server --push
```

默认行为：

- 读取 `Cargo.toml` 里的版本号作为镜像 tag
- 额外再打一个 `latest` tag
- 默认构建 `linux/amd64`

如果你要多平台发布，可以显式传平台参数：

Windows:

```powershell
./scripts/docker-release.ps1 -Image namelsscinder/floral-sync-server -Platform linux/amd64,linux/arm64 -Push
```

Linux / macOS:

```bash
bash ./scripts/docker-release.sh --image namelsscinder/floral-sync-server --platform linux/amd64 --platform linux/arm64 --push
```

如果你只是想本地验证镜像，不推送仓库，直接去掉 `-Push` / `--push` 即可。此时脚本会把镜像加载到
本地 Docker daemon，方便你立刻用 compose 运行。

## 首次启动

把二进制复制到目标机器后，先运行一次：

```bash
./floral-sync-server config show
```

如果同目录还没有 `sync-server.toml`，服务端会自动创建它，并生成一个随机 Token。

默认情况下：

- 同步接口监听 `0.0.0.0:8787` 和 `[::]:8787`
- 管理后台监听 `127.0.0.1:8788` 和 `[::1]:8788`

管理员密码默认未配置，所以第一次打开 `http://127.0.0.1:8788/admin` 时，
会先进入一次性引导流程来设置后台密码。

## 写入正式配置

推荐在部署机器上直接用命令行写配置，而不是手动编辑。

### 同时监听 IPv4 和 IPv6

```bash
./floral-sync-server config set \
  --listen 0.0.0.0:8787 \
  --listen [::]:8787 \
  --db /srv/floral-sync/data/floral-sync.sqlite3 \
  --generate-token
```

### 只监听 IPv4

```bash
./floral-sync-server config set --listen 0.0.0.0:8787
```

### 只监听 IPv6

```bash
./floral-sync-server config set --listen [::]:8787
```

如果你希望把配置文件放到固定路径，例如 `/etc/floral-sync/sync-server.toml`：

```bash
./floral-sync-server \
  --config /etc/floral-sync/sync-server.toml \
  config set \
  --listen 0.0.0.0:8787 \
  --listen [::]:8787 \
  --db /srv/floral-sync/data/floral-sync.sqlite3 \
  --generate-token
```

## 启动服务

### 直接启动

```bash
./floral-sync-server
```

### 使用自定义配置文件启动

```bash
./floral-sync-server --config /etc/floral-sync/sync-server.toml
```

### 只在当前进程临时覆盖配置

```bash
./floral-sync-server \
  --config /etc/floral-sync/sync-server.toml \
  --listen 127.0.0.1:8787
```

上面的 `--listen` 只影响这次启动，不会改动配置文件。

## 管理后台

管理后台默认只监听本机回环地址，推荐保留这个默认值；如果要暴露到局域网或公网，
请放到 HTTPS 反向代理后面，并把访问入口限制到你自己的管理域名。

后台支持：

- 首次引导设置管理员密码
- 查看同步概览和只读笔记快照
- 在设置页显示、复制和轮换当前同步 Token
- 在托管环境里请求服务重启
- 通过维护页创建 SQLite 备份和查看日志尾部

如果你在 NAS 上仍然看到类似 `Permission denied` 的错误，先确认宿主机挂载目录对 Docker 运行环境可写。
当前镜像会在首次启动时自动准备挂载目录并生成配置文件，但宿主机共享目录本身的 ACL 或只读挂载仍会阻止写入。

## systemd 示例

```ini
[Unit]
Description=Floral Sync Server
After=network.target

[Service]
WorkingDirectory=/srv/floral-sync
ExecStart=/srv/floral-sync/floral-sync-server --config /srv/floral-sync/sync-server.toml
Restart=always
RestartSec=3
User=floral
Group=floral

[Install]
WantedBy=multi-user.target
```

## Caddy 示例

```caddyfile
notes.example.com {
    reverse_proxy 127.0.0.1:8787
}

notes-admin.example.com {
  reverse_proxy 127.0.0.1:8788 {
    header_up Host {host}
    header_up X-Forwarded-Proto {scheme}
  }
}
```

桌面端把服务器地址配置成 `https://notes.example.com`，并填写同步 Token。
浏览器访问 `https://notes-admin.example.com/admin` 打开后台。

管理后台的 POST 接口会校验 `Origin`，所以反向代理必须保留外部 `Host`，并传递
`X-Forwarded-Proto`。

## Nginx 示例

```nginx
server {
    listen 443 ssl http2;
    server_name notes.example.com;

    ssl_certificate /path/to/fullchain.pem;
    ssl_certificate_key /path/to/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
    }
}

    server {
      listen 443 ssl http2;
      server_name notes-admin.example.com;

      ssl_certificate /path/to/fullchain.pem;
      ssl_certificate_key /path/to/privkey.pem;

      location / {
        proxy_pass http://127.0.0.1:8788;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
      }
    }
```

## 备份

重点备份配置文件里的 `db_path` 指向的 SQLite 数据库。SQLite 是服务端唯一状态来源。
客户端虽然也保存本地 Markdown 文件，但数据库里还包含删除墓碑以及保证增量同步正常工作的
服务端 revision。

最简单的备份方式是停掉服务后直接复制数据库文件。需要在线备份时，可以使用
SQLite 的 `.backup` 命令。

如果你已经启用了后台，也可以直接在维护页触发一次备份导出。服务端会把备份文件写到
配置里的 `export_dir`。

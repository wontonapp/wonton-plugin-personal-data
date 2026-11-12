# 手机数据：手机的位置和健康数据存在自己家里

存手机同步来的位置点和健康样本，给家里的助手查历史、按期限清理、按人导出。数据只进自己的服务器，按人分开存。

开发流程见[插件开发指南](https://github.com/wontonapp/wonton-docs/blob/main/develop.md)，清单字段以[插件规范](https://github.com/wontonapp/wonton-docs/blob/main/plugin-spec.md)为准。

## 文件

| 路径 | 内容 |
| --- | --- |
| `docker-compose.yml` | 清单、存储服务、数据目录和健康检查 |
| `src/` | 存储服务：SQLite、契约的六个接口、凭据命令 |
| `images/store/Dockerfile` | 自建镜像，两段构建，运行镜像只有二进制 |
| `scripts/smoke-probes.sh` | 装好以后真写一条位置、再读回的探针 |
| `screenshots/` | 商店详情页截图 |

## 接口

契约在核心仓库 `wonton-protocol` 的 `personal_data` 模块，核心经能力 `personal_data` 转发或直连：

| 方法与路径 | 做什么 |
| --- | --- |
| `POST /location` | 收一个位置点（OwnTracks 的 `_type: location` 写法） |
| `POST /health` | 收一批健康样本，带同步到的时刻 |
| `GET /query?type=&from=&to=` | 查时间段内的点或样本 |
| `DELETE /data?type=&from=&to=` | 删时间段内的数据 |
| `GET /export` | 这个人的全部数据 |
| `POST /cleanup` | 按保存期限删到期数据 |
| `GET /health` | 容器健康检查 |

每个请求带 `X-Wonton-User`（数据主体）和核心按使用方签发的凭据（`Authorization: Bearer`）。未知的类型按健康类型收，多余的字段原样存、原样回。

## 数据

SQLite 在 `${WONTON_APP_DATA}/store.db`，人和类型分开存；重传同一个点或同一个健康来源只留一份。`never` 的类型收下就删。

## 图标

`icon.svg` 为本仓库原创线条图标，按本仓库 MIT 许可证分发，见 [NOTICE](NOTICE)。

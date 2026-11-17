# Core + DSH 插件测试报告

日期：2026-11-17（Asia/Shanghai）。本轮清理开发期旧版本兼容，并将双模式实现作为当前唯一契约。历史结果另见 [2026-11-07 归档](<archive/TEST_REPORT_20261107.md>)，不作为本轮证据。

## 清理范围

- 模型路由仅 `local_only` / `cloud_assisted`；删除旧模型路由、全局 `private_notes`、旧名单空值/通配授权旁路。内容 `DataClass` 及其云端等级保留。
- 策略拒未知字段，命令必须显式给出 `operation_version`；测试过期格式的拒绝，而不维持迁移分支。
- 删除 A0 probe crate、发布依赖与二进制声明；删除旧右 sidebar 注册及客户端依赖。Host 健康信息从现行 manifest 读取版本与需求，不冒充锁定旧 Host。
- 15 个不维护 Python 测试、31 个旧浏览器脚本及 3 个固定旧路径诊断工具归档；不为旧入口维护当前产品。11 个现行准入脚本仍仅由隔离 runner 执行，cloud stub 已审查并改用当前模式。

## 本轮实际执行

| 层级 | 结果 | 入口 / 证据 |
|---|---|---|
| Rust workspace | **100 通过，0 失败** | `cargo test --workspace --locked --offline` |
| Rust 格式 | **通过** | `cargo fmt --all -- --check` |
| 隐私及发布范围 guard | **19 通过，0 失败** | `node --test tests/adapter/privacy.test.mjs tests/adapter/ui-scope.test.mjs` |
| 隔离 runner 安全 guard | **9 通过，0 失败** | `python -B -m unittest discover -s tests/integration -p test_runner_safety.py -v` |
| HTTP / 跨模块集成 | **12 场景组通过** | [本轮 API 报告](<../tests/integration/runs/isolated-13fhnuxo/summary.json>) |
| React / bridge 回归 | **28 场景组通过** | [本轮浏览器报告](<../tests/browser/harness-fixture-runtime/results/2026-11-17T02-53-19-718Z/report.json>) |
| 左 sidebar 注册 | **通过**，只使用 slots，无旧右 tab | `node tests/browser/zodo-sidebar-registration.mjs` |
| Release workspace | **构建成功** | `cargo build --release --workspace --locked --offline` |
| 差异格式 | **通过** | `git diff --check` |

不同层级的“测试”“检查”“场景组”不叠加成一个总数。路由矩阵在测试内部覆盖 288 个组合。

本轮 Core SHA-256：`d45cb76da7b3b047128fef66f5168713b362d11bd5254fa52eac140065157a6b`。

## 重点覆盖

- 两模式、内容敏感度、角色、显式名单、凭据与本地开关的路由组合；空名单和 `*` 不授权云端。
- DeviceOnly / 仅本地不因云端角色偏好而联云；缺 Key 在请求前选本地或明确失败；loopback 固定解析、禁止代理与 redirect。
- 三种笔记数据等级在模式切换中不变；Agent 仍不能改策略、批准或提交自己的提案。
- 首次云端授权、连接与模式独立保存、缺 Key 状态、保存失败不误报成功、过期草稿检测与放弃重载。
- 旧字段/旧路由拒绝且策略与笔记不变；错误输入不会触发自动迁移。
- 21 个导航入口在 390px、800px 和 600px 容器中可达且无横向溢出；高级设置可键盘开合。
- 浏览器无页面脚本错误、无非预期外部浏览器请求；自有 Core/server 均停止，临时数据删除。

初轮浏览器测试中的 409 是 fixture 直接创建笔记后 React 基线未刷新，已修正测试 setup 先同步 workspace revision；保留产品的冲突拒绝，不通过移除版本检查让测试变绿。

## 边界

全部自动化使用合成数据及自有 loopback 服务，不证明真实云端推理成功，也不覆盖完整 DSH 登录、Host 路由与 Electron 转发、真实隐私历史生命周期或所有无障碍状态。真实安装更新与自动化 fixture 是两件事；安装结果以实际 Core 健康、二进制哈希和数据核对为准，不用另起服务器冒充当前 GUI。

开发版本不提供旧数据结构自动迁移。更新已有开发实例时应先备份，再明确重置过期设置并核对实体不变；不要清空用户任务或笔记。

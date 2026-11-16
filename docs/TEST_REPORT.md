# Core + DSH 插件测试报告

日期：2026-11-07（Asia/Shanghai）。结论：**本轮隔离自动化回归通过；不等于完整产品、全部状态或真实安装环境验收。**

## 范围清理

按本轮用户指令，仅保留 DSH 插件 UI。独立客户端源码、平台工程、专用代理、Dart 生成器、客户端测试及对应发布包已删除；构建/回归入口不再依赖独立 UI。独立 Rust Core、CLI、MCP、C ABI 与协议契约保留。原设计和历史证据不删改。

[范围决策](<../implementation/SCOPE_DECISION.md>)明确：台账中 164 个历史 verified 不代表本轮通过；6 个退役客户端条目标为 out_of_scope，不计作测试成功。

## 实际结果

| 层级 | 本轮结果 | 证据 |
|---|---|---|
| Rust workspace | 96 通过，0 失败/忽略；新增 12 项 | [Core 结果](<../artifacts/evidence/regression-20261107/core-results.json>) |
| 隐私与 UI 范围 guard | 19 通过，0 失败 | [适配器结果](<../artifacts/evidence/regression-20261107/adapter-final-results.json>) |
| 隔离 runner 安全 guard | 8 通过，0 失败 | [安全结果](<../artifacts/evidence/regression-20261107/safety-results.json>) |
| HTTP/跨模块集成 | 12/12 场景组通过；其中新鲜 API 50/50 检查通过 | [集成结果](<../artifacts/evidence/regression-20261107/integration-results.json>) |
| DSH React UI | 20/20 场景组通过；21 页桌面导航 + 63 次响应式导航点击 | [UI 结果](<../artifacts/evidence/regression-20261107/ui-results.json>) |
| JS 静态/注册 | 三个插件模块语法检查、侧栏注册与卸载检查通过 | [原始适配器记录](<../artifacts/evidence/regression-20261107/adapter-results.json>) |
| 回归脚本契约 | 原生 stderr + exit0 不误报失败；exit7 正确判失败 | [脚本契约结果](<../artifacts/evidence/regression-20261107/runner-wrapper-results.json>) |

“组”“检查”“Rust test”不是同一种计数，不叠加成虚假的总测试数。

Core release SHA-256：`9702ccedaa0511b492bb5e69c8e380f2d12a7df3646e4043b568ab03a0e7af17`。API 和最终 UI 均使用该修复版；UI 证据另绑定 bridge/client 源码哈希。

## 构建与发布包核验

Lead 独立运行了默认统一回归入口：7 个步骤全部 exit0（包含再次执行 96 项 Rust 与 19 项适配器测试），见 [统一入口回执](<../artifacts/evidence/regression-20261107-225559-368/summary.json>)。随后 [发布脚本](<../scripts/build-release.ps1>) 实际执行成功，完整 workspace release 增量构建通过。

[发布元数据](<../artifacts/release/BUILD_INFO.json>) 明确记录 dirty working tree，UI 为 DSH plugin only；没有把未提交改动冒充干净提交。对 [插件 ZIP](<../artifacts/release/zodo-harness-plugin-0.1.0-a0.zip>) 的二进制核验通过：11 个条目，插件源码逐字节一致，Core SHA 与 API/UI 所测版本一致，privacy 模块、locale、probe 均存在，没有独立客户端、运行时配置或 SQLite 数据。见 [包核验结果](<../artifacts/evidence/regression-20261107/package-results.json>)。

## 功能与 UI 对照

| 模块 | 功能层 | 对应 UI 实际覆盖 |
|---|---|---|
| 收集、任务、目标、项目 | 校验、幂等、层级/依赖、状态与转换 | 收集/澄清/转换，任务编辑/完成/重开/归档/恢复，目标及关联项目创建，搜索 |
| 规划、容量、日历、重复 | 确定性、锁定/过去工作保护、边界、导入/写入 stub、重复规则 | 容量创建，提案预览不写事实，显式批准提交，分配与 revision 强断言；非所有重复编辑 UI 状态 |
| 习惯、视图、自动化 | 去重、筛选、预算、定时与手动运行 | 习惯创建/记录，视图创建/应用，手动自动化耗尽预算且不重复写入 |
| 专注、进度、回顾 | 单实例专注、状态/记录/恢复、回顾派生事实 | 专注开始/暂停/继续/结束，结束不完成任务；回顾页导航，未穷尽回顾编辑状态 |
| 数据、备份、同步 | 导入 preflight/重放/冲突、合成备份恢复、双设备及加密同步/撤销 | JSON/CSV 下载、SQLite 备份哈希；同步页导航，实际同步交互主要在 API 层验证 |
| AI、模型、权限、审计 | loopback 模型 stub、任务/依赖计数、授权/撤权/跨工作区拒绝、批准防重放、事件游标 | Host 模型目录只读、填草稿不保存、失败重试、运行/隐私/Agent/审计页导航；非真实模型推理或真实 Host 历史链 |

UI 三种响应式场景：390px、800px，以及 1440px 视口中的 600px 插件容器。各场景逐页点击 21 个入口，检查激活状态、标题和文档/导航横向溢出。还测试空状态、表单校验、Escape/焦点返回、目录失败恢复、离线草稿保留与重连防重复。

## 修复的问题

1. **AI 任务数误计依赖**：3 个任务 + 2 条依赖原先显示 5 个任务。已在 [runs.rs](<../crates/zodo-runtime/src/runs.rs#L718-L828>) 于追加依赖前保留有效任务数，同时修正提案描述及运行输出。红测 1 失败 → 修复后两项定向测试通过，随后 96 项全量及模型 stub 集成通过。
2. **窄屏隐藏业务入口**：媒体样式隐藏全部次级导航，设置等页面不可达。已在 [DSH client](<../adapters/deepseek-harness/plugin/client.js#L5177-L5206>) 保留入口并换行，三种尺寸全部点击通过。
3. **发布/回归维护缺口**：[构建脚本](<../scripts/build-release.ps1>) 仅打包 Core + DSH UI，并补齐 privacy 模块及声明的 probe 二进制；[回归脚本](<../scripts/run-regression.ps1>) 不再读取旧 live URL，使用自建隔离场景，修正 Windows PowerShell 原生 stderr 误判问题。

两项早期失败是测试预期错误（提案状态实际为 applied；Focus 的隐式 label 导致 Playwright exact 匹配失败），已依据 Core enum/实际 ARIA 修正测试，保留业务写入、revision 与生命周期强断言，未修改产品行为掩盖失败。

## 视觉证据

Lead 已查看自动化截图：

- [800px 全部导航](<../artifacts/evidence/regression-20261107/navigation-800.png>)、[390px 导航](<../artifacts/evidence/regression-20261107/navigation-390.png>)
- [计划预览/明确批准](<../artifacts/evidence/regression-20261107/plan-preview.png>)
- [专注结束记录](<../artifacts/evidence/regression-20261107/focus-completed.png>)、[离线草稿](<../artifacts/evidence/regression-20261107/offline-draft.png>)

共享浏览器 lane 被其他会话占用，未绕过占用或操作用户桌面。以上为现有 Playwright 测试产生的隔离截图复核，不声称完成当前已安装 DSH 的人工端到端测试。

## 安全与尚未覆盖

- 数据全部为新建临时/唯一 fixture，未操作真实任务、canonical Core 或冻结归档；本任务自有 Core/浏览器/HTTP 服务已退出，UI 临时数据已删除。API 原始合成数据库/本任务 token 留在 Git 忽略的本地证据树，不作为交付物。
- 真实 DSH 登录、路由/侧栏组合、Electron 转发、真实 Host 隐私投影持久化/分叉/压缩、安装卸载及热生命周期未由 fixture 证明。
- 未调用真实/付费模型、真实日历提供商或发送系统通知。15 个 legacy 脚本明确跳过；部分能力由新 fixture 替代覆盖，而非原脚本已运行。具体见 [集成独立报告](<../tests/integration/RESULTS.md>)。
- 模型取消竞态、真实崩溃恢复、完整 privacy 路由矩阵，以及每个页面/弹窗的所有故障与无障碍状态仍需专门 fixture。没有覆盖率百分比、非 Windows 或真实屏幕阅读器验收结论。
- 没有将源码/发布包自动安装到当前运行 GUI，避免无授权重载及数据影响。

## 重跑

安装锁定的浏览器/fixture 依赖后，在具备 Rust/MSVC 环境的仓库根目录运行：

```powershell
powershell -NoProfile -File scripts/run-regression.ps1 -WithIntegration -WithBrowser
```

单独的复现与范围说明见 [UI fixture 说明](<../tests/browser/harness-isolated-README.md>)、[API fixture 说明](<../tests/integration/README.md>)。默认回归与可选 API/UI 场景分别留存输出，不把历史台账当作新结果。

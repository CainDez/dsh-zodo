# 当前测试报告

日期：2026-11-17（Asia/Shanghai）。本轮以单一 DSH/自定义模型来源与独立隐私授权取代双模式设计。插件版本：0.3.0-dev.1。当前契约见 [模型与隐私](<MODEL_SETTINGS.md>)；上一轮结果另见 [当日上午归档](<archive/TEST_REPORT_20261117_MODES.md>)，不作为当前设计的证据。

## 当前设计

- 两个菜单打开两个页面：模型只保存 source，隐私只保存 model_access 与诊断开关，互不修改对方。
- 模型严格 version:2，唯一 source.kind 为 dsh/custom；默认 DSH 未选，不继承聊天模型，无 local/cloud 双槽、角色或跨模型 fallback。
- 目标内容、项目背景、收集原文分别授权，默认全部关闭；项目未授权就裁剪而非逼用户开启。
- DSH 由实际 ctx.llm.stream 执行，凭据留在 Host，system/user 之外不传聊天历史、session 或工具。模型保存会更新当前可调用路由，避免新加入的模型只能等绑定过期才用。
- Host 凭据与一次性派发许可仅内存；绑定实例、模型、策略版本、run、scope、正文摘要，拒绝伪造、重放与过期许可。自定义连接使用可取消成熟 HTTP client，无临时自制 relay、环境 proxy 或 redirect。

## 已完成验证

| 项目 | 结果 |
|---|---|
| Rust workspace | 110 项通过（FFI 3、contracts 13、runtime lib 28、a1 11、a4 50、runs 2、recurrence 3） |
| Host gateway 单测与真实 Core 全链路 | 6/6 通过，最终 release Core |
| 原隐私投影及独立 UI 范围 | 22/22 通过 |
| 隔离 runner 安全契约 | 9/9 通过 |
| HTTP 与跨模块集成 | 12 组通过，含自定义 stub 实际模型请求与两条任务依赖提交 |
| 隔离浏览器 | 28/28 通过，21 个导航页面，最终包含上下文预览权限修正 |
| 原生 sidebar/main 注册检查 | 通过 |
| Rust fmt / diff 空白检查 | 通过 |
| 两设置页视觉体检 | 390×844、1280×900 均通过；修复小点击区、字号及折叠内容检测问题 |
| Release workspace build | 通过 |

可重跑入口：[Host gateway tests](<../adapters/deepseek-harness/plugin/host-gateway.test.js>)、[browser regression](<../tests/browser/harness-isolated-regression.mjs>)、[integration runner](<../tools/run_isolated_integration.py>)。API 本轮结果位于 [summary](<../tests/integration/runs/isolated-ohf5y3zo/summary.json>)；浏览器最终结果见 [report](<../tests/browser/harness-fixture-runtime/results/2026-11-17T07-14-16-676Z/report.json>)。

## 验证到哪一层

Host 测试不是只测目录：真实临时 Core run → 真实 Host gateway → 普通与订阅样式 provider 的确定性 llm stub → proposal。测试确认项目未授权时 title/scope/ID 不进入 messages，DeviceOnly note 不混入、goals/captures 拒绝零派发、缺 Host 不 fallback、模型只产候选、Agent 不能审批。另覆盖 registry 等待时撤权、在途取消、响应限额、旧 source/policy 拒绝与新目录刷新。

没有调用真实 ChatGPT/OAuth 或付费模型，没有读取真实 provider credentials；mock 结果不等于真实账号的认证、配额或推理质量验收。目录和预检状态也不表示模型请求已成功。

隐私三个开关仅控制 Zodo 内置拆解/澄清；DSH 主动工具、文件、导出、手动粘贴仍走各自权限和资料等级。UI 对此明确说明，不将其称为全局数据防火墙。

## 实际安装记录

实际 Core 已经一致性备份后替换并经原 Host 健康端点重新发现。已核对业务实体内容哈希不变；过期模型/策略明确重置为 DSH 未选择、三个资料权限关闭，而非保留兼容迁移代码。旧模型配置与旧二进制也保存在用户数据备份目录，未进入 Git 或发布包。

测试与安装 Core SHA-256：1d04852ca7399021e61dc77b3d9fc65ce5dd4190fe42b8ff42c45e4ada536bc1。

安装链接中的 manifest 已是 0.3.0-dev.1；正在承载本会话的 DSH Host 仍报告缓存的 0.2.0-dev.1，未强制关闭。需完整退出并重新打开 DSH 加载新版 Host/UI。未把隔离预览冒充实际桌面 GUI 已更新。

# 03 · 运行时、契约与平台

状态：设计规格。接口与示例尚未实现，平台行为须逐项验证。

## 1. 物理布局与依赖

采用 Rust 模块化单体与薄适配器；这里是拟议目录，不代表工程已创建。

| 模块 | 职责与阶段 |
| --- | --- |
| crates/zodo-kernel | domain/application/intelligence/ports；领域智能、约束、用例与提案；无 Flutter/Cordis/React/DB/MCP SDK |
| crates/zodo-runtime | SQLite、模型、Runs、effects、凭据与统一API；lib和zodo可执行文件 |
| adapters/deepseek-harness/host | 树外TypeScript/Cordis服务、原生工具、UI RPC桥接、身份与连接管理 |
| adapters/deepseek-harness/client | React/Slots、Client model和工作台；只消费已授权投影与命令 |
| contracts/v1 | Zodo命令/查询/结果/错误/事件schema及正反例；版本独立于宿主 |
| crates/zodo-ffi 与 apps/flutter | 后续独立客户端的C ABI、KernelClient与跨平台展示；保持规划，非首期插件前置工程 |

依赖经统一应用入口进入domain，runtime通过ports注入存储/时钟/模型。共享契约由Zodo拥有并生成或校验Rust/TypeScript/Dart类型。Host Remote生成物仅是宿主边界，不反向定义Core领域或授权。

## 2. Runtime部署与首期宿主连接

每用户、每数据目录一个受认证的Rust daemon，持有SQLite、写队列、模型worker与耐久Jobs。Harness Host、CLI、MCP及后续Flutter连接同一实例。以OS锁、实例nonce、工作空间及版本握手核实实例，不只凭PID文件；后台运行/退出策略由用户控制。

首期连接可采用经过认证的loopback HTTP与事件流；校验Origin、端点所有者、workspace、实例nonce与访问凭据，并限制资源。按需求扩展Windows named pipe或Unix socket。浏览器先调用宿主公开的Remote/RPC面，再由Host桥接Core；React不持有Core全权token或直接读SQLite。

人工UI与Agent调用使用独立授权视图。scope、批准与actor由可信连接及Core授权导出，不能信任模型payload里的actor、approved或角色。工具执行许可不代替当前业务提案批准。插件热重载只清理自身订阅/连接，不能顺手删除业务状态或无条件停止共享daemon。

Host与Core可分别重启；以持久快照、receipt、Run和事件游标恢复，重连不重发未知结果的写操作。宿主分叉或回放仅恢复会话引用和显示，不回滚或再次执行Zodo外部效果。失效来源重新鉴权，旧批准不能用于新内容。

后续原生移动端把Rust嵌入应用沙盒，不承诺系统挂起后持续推理；提醒/安全存储/恢复走平台能力。后续Flutter Web以认证API连接runtime，离线仅缓存与未提交草稿。宿主浏览器、Host机器与Core所在设备分别标明，不能统称本地。

## 3. 共享契约与完整调用形状

各入口映射同一操作。HTTP 拟为 /v1/commands、/v1/queries、/v1/events；MCP 使用领域工具及严格 Schema。协议、Schema、operation_version、DB 迁移号独立演进。

下例为拟议外部 command，grant 为不透明标识；字符串中的示例 ID 不表示真实数据：

~~~json
{
  "schema": "zodo.command/1",
  "request_id": "req_123",
  "idempotency_key": "op_456",
  "workspace_id": "ws_local",
  "operation": "changes.commit",
  "operation_version": 1,
  "base_revision": 42,
  "policy_revision": 7,
  "payload": {
    "proposal_id": "prop_789",
    "approval_grant": "grant_abc"
  }
}
~~~

request_id 关联传输，idempotency_key 去重。Query 使用 zodo.query/1，不能写入。Mutation 必带基础/策略版本。外部 actor、role、scope 等保留字段拒绝；可信身份只能由 runtime 注入。

传输完成认证后，由 runtime 另行注入如下内部上下文：

~~~json
{
  "actor": {
    "principal_id": "integration_17",
    "kind": "delegated_agent",
    "grant_id": "access_92"
  },
  "channel": "mcp_stdio",
  "runtime_instance": "rt_12",
  "hash_version": 1,
  "command_hash": "<runtime-computed-sha256>",
  "received_at": "2026-10-24T01:30:00Z"
}
~~~

用户与各 integration 分开授权；clientInfo、模型自报身份不构成认证。

~~~json
{
  "schema": "zodo.result/1",
  "request_id": "req_123",
  "operation_id": "op_456",
  "status": "committed",
  "revision": 43,
  "data": {
    "change_set_id": "cs_18",
    "changed_task_ids": ["task_6"],
    "effects": [{"id": "eff_2", "status": "pending"}]
  },
  "event_cursor": "ev_109",
  "replayed": false
}
~~~

~~~json
{
  "schema": "zodo.error/1",
  "request_id": "req_124",
  "error": {
    "code": "REVISION_CONFLICT",
    "message": "计划所依据的数据已改变",
    "retryable": false,
    "details": {
      "expected_revision": 42,
      "current_revision": 44,
      "recovery": "refresh_and_repropose"
    }
  }
}
~~~

稳定 code 另含 VALIDATION_FAILED、POLICY_CHANGED、PERMISSION_DENIED、APPROVAL_REQUIRED/EXPIRED、IDEMPOTENCY_MISMATCH、RESOURCE_EXHAUSTED、RUN_NOT_FOUND、PROVIDER_UNAVAILABLE。按 code 恢复；未知 Schema/operation 先拒绝，语义/必填变化升版本，未知 enum 不默认为成功。

HTTP 映射 400/401/403/409/422/429/503；FFI/CLI 保留业务 code，CLI 使用非零失败退出码。MCP 业务失败用 isError，消息错误用 JSON-RPC error；传输完成不代表提交成功。

## 4. 幂等、版本与批准绑定

唯一键为 workspace_id + principal_id + idempotency_key。对校验并填默认值后的 command 计算版本化 canonical hash：含 operation/version、payload、base_revision、policy_revision，排除 request_id/trace/时间。规范固定键排序、数字、缺失/null；不擅自归一化任务文本，保留 hash_version。

同 key/hash 返回原 receipt/Run 并标 replayed；不同 hash 拒绝；进行中返回相同 operation_id。修正 proposal 用新 key。记录与提交同事务；重放仍查当前权限。过期键留 tombstone，不静默作为新写入。

初版使用工作空间单调 revision，提交查 base_revision，冲突返回；实体级优化后续再做。创建 proposal 与提交均查 policy_revision。

批准来自可信用户入口或委托策略，不接受 approved:true。grant 绑定主体、工作空间、proposal hash、基础/策略版本、effects、有效期和单次消费状态；内容变化重新批准，已提交重试仅返 receipt。

可撤销动作可预授权；删除/批量/外发按策略处理。host 确认 UI 不替代 Kernel 检查；批准入口与 agent 凭据分离，不承诺隔离同 OS 用户恶意进程。

## 5. SQLite 与外部 effects

单写队列之外仍需事务；读取使用独立只读快照。选择本地磁盘、WAL 与 busy timeout，禁止网络共享盘。SQLite 多读单写，WAL 需管理 checkpoint。[SQLite 事务](https://sqlite.org/lang_transaction.html)、[WAL](https://sqlite.org/wal.html)

一次 changes.commit 的短事务：BEGIN IMMEDIATE → 复核授权/版本/批准 → 变更领域表 → 写 operation receipt、audit/event、outbox → 消费批准 → revision 加一 → COMMIT。模型推理与远程调用均在事务外；任一步失败整体回滚，不出现批准已消费但任务未修改。

拟议表包括 tasks、proposals、runs、run_steps、operation_receipts、approval_grants、policy、audit_events、outbox。当前状态读领域表，不做完整 event sourcing。迁移由唯一写者执行，先一致备份，拒绝不兼容客户端。

outbox 提交后重新检查 effect 授权，以 effect_id 去重并保存重试状态。外部结果未知且无法查询/幂等时转 needs_reconciliation，不盲目重发。分别展示本地提交与外部送达；两者不跨系统原子化。

## 6. Runs、事件与取消

Run 区别于用户 Task/MCP task。长操作先持久化 queued+输入快照再返 run_id；状态为 queued → running → waiting_input → succeeded/failed/cancelled。保存模型/提示版本、参考时间/时区、预算、checkpoint、数据版本及摘要，不存隐藏思维过程。

worker 写 lease/heartbeat，重启将过期执行标 interrupted 并分类恢复：计算可重算，已确认 effect 查 receipt，未知结果先对账；不承诺恢复模型 KV cache。恢复重新检查策略与权限。

run.cancel 先存 cancel_requested，再停止排队并通知 worker。事务前观察到取消则不提交；已提交保留 committed，补偿是新操作。不可中断的计算可以结束，但不能越过取消边界发起新 effect。

events 用持久 cursor 补拉，resync_required 时拉快照。token 草稿可丢，提交/批准/终态必须持久化。实时预览用 input_revision、去抖与最新请求合并，不逐键创建 Run，旧结果不覆盖新输入。

## 7. 后续 Flutter FFI 与 KernelClient

以下是后续客户端的既有技术基线，A5实现时重新核对和锁定依赖；不阻塞首期Harness插件。Flutter 官方自 3.38 起推荐 package_ffi + hook/build.dart；hook 产出 code assets，Dart 通过 @Native 绑定。Dart hooks 明确支持调用原生编译器生成 Rust assets。优先验证此路径，不能默认回到 legacy plugin_ffi，也不能据官方 C 示例宣称 Rust 打包已通过。[Flutter FFI](https://docs.flutter.dev/platform-integration/bind-native-code)、[Dart Hooks](https://dart.dev/tools/hooks)

拟议 Rust 边界使用 C ABI 与 cdylib，隔离 Rust 类型、panic 和 allocator；每块返回内存由同一 Rust free 函数释放。FFI 提供 open/close、submit、poll_results、poll_events、cancel；入口快速入队，推理与磁盘工作在 Rust worker，Dart UI 线程不阻塞等待。首版批量轮询事件，避免 Rust 任意线程直接调用 Dart callback；后续需要推送时再验证线程安全方案。[Rust linkage](https://doc.rust-lang.org/reference/linkage.html)

hook 按目标 ABI 调用 Cargo/NDK 或校验预编译产物 SHA256，不能打包宿主库。Apple framework 名/asset ID 跨设备和模拟器一致；签名先 PoC，确需静态插件时记录例外。

~~~dart
// 拟议接口；DTO 由 contracts/v1 约束，当前不编译。
abstract interface class KernelClient {
  Future<KernelInfo> connect();
  Future<QueryResult> query(QueryEnvelope query);
  Future<CommandResult> execute(CommandEnvelope command);
  Stream<KernelEvent> watch({String? afterCursor});
  Future<RunSnapshot> getRun(String runId);
  Future<CancelReceipt> cancelRun(String runId);
  Future<void> close();
}
~~~

Ipc/Embedded/HttpKernelClient 共用 DTO、错误、cursor；UI 只缓存投影，断线恢复留在 client。KernelInfo 声明 local_inference/remote_model/durable_runs，界面按能力显示操作。

## 8. MCP、模型与隐私

既有协议基线采用 MCP 2026-07-28，实现时重新核对官方规范，无现代 HTTP session/initialize；兼容 2025-11-25。runtime 用官方 rmcp，锁版后测试双时代 stdio/HTTP。Tasks 双方 opt-in，缺失时以 run_get/run_cancel 回退；不依赖 elicitation/MCP Apps。sampling 已弃用，直接走 ModelGateway。[MCP 版本](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)、[rmcp](https://github.com/modelcontextprotocol/rust-sdk)、[Tasks](https://modelcontextprotocol.io/extensions/tasks/overview)、[弃用项](https://modelcontextprotocol.io/specification/2026-07-28/deprecated)

ModelGateway 区分 local/cloud；local_only 禁止云回退，失败明确显示。device_only 还禁止把材料送往远程 runtime；trusted_workspace 只允许用户明确授予的执行边界。cloud 仅发送授权的最小上下文，路由/保留/凭据单独配置。外部 agent 可将读取内容传给其模型，“本地 MCP”不保证数据本地，授权需展示工作空间、字段与范围。

HTTP 远程入口使用 TLS、正确 audience 的 OAuth token 与领域 scopes；本地 stdio 使用预配置 integration grant，不把所有 agent 当作 UI 用户。远程接口不接受任意数据库路径、shell 或 URL 作为通用执行能力。[MCP HTTP 授权](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)

key 存系统凭据库/部署 secret，DB 仅 credential_ref，浏览器不存 provider key。Zodo自有日志只含 ID、耗时、状态、模型版本/计量和脱敏错误，不含正文/prompt/token/授权头/签名 URL。原文诊断需主动开启、限时可删；结果按权限裁剪。进入Harness Agent的内容可能进入其会话日志；按会话可接受的保存/外发边界裁剪，不仅检查当前模型路由。

## 9. 构建与验收矩阵

下表均待验证。首期先验证宿主插件与Rust运行时交集；后续Flutter分别完成原生/浏览器客户端。版本锁定记录Host版本/commit、Node、构建辅助、插件包、Core协议与二进制身份。

| 目标 | 部署阶段 | 必须通过的实证 |
| --- | --- | --- |
| Windows x64 Harness | 首期：匹配Host、Node与Rust二进制 | 树外安装/加载、认证桥接、热重载、双客户端、Host退出/Core重启、真实模型数据流 |
| macOS arm64 / Linux x64 Harness | 目标平台逐一验证 | Core二进制分发、Host兼容、权限与进程生命周期、升级恢复；不得用Windows结果替代 |
| Harness Web Client | 浏览器→Host→Core | 信任/Origin与身份、取消、事件重连、过期会话；离线草稿不能显示已提交 |
| Flutter桌面 | 后续A5 | 独立启动、相同契约、可达性、安装升级、后台及通知 |
| Android / iOS原生 | 后续A5，SDK/NDK或Apple工具链 | FFI入包、release ABI/签名、真机内存、挂起重开、通知与安全存储 |
| Flutter Web | 后续A5，认证远程runtime | 浏览器兼容、断网草稿、身份失效、事件恢复；无浏览器本地Core或FFI承诺 |

## 10. 实现前 P0 出口

A0先证明公开运行时契约下的树外Host+Client插件能构建、安装、加载并与Core握手；开发文档中的未发布构建预设不视为已可用SDK。可按许可固定源辅助或自行产出契约要求的bundle，必须记录来源并实测，不默默fork宿主业务源码。A1–A3验证Rust/CLI/Harness/MCP共享事实；Flutter/FFI/Apple签名等在A5独立验收。缺少凭据、runner或设备保留为未验证，不可用其他平台或mock结果代替。

故障注入覆盖崩溃、过期批准、幂等冲突、并发修改、取消、未知送达与撤权恢复。记录工具链、目标、命令、退出码和证据；未测平台标“未验证”，不能从支持列表推导已可交付。

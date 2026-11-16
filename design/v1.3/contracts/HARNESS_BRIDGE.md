# Harness Bridge 设计级契约

状态：1.3 拟议适配，尚无已实现 SDK 或完整 IDL。本文件补充 [Core 运行契约](../spec/03-runtime-contracts-and-platforms.md) 与 [Harness 架构](../spec/06-harness-adapter-and-ui.md)，只定义边界和映射；操作名、字段及 bridge 错误须在实现初期冻结为共享 Schema，并做 Rust/TS contract tests。

## 1. 两跳通信与身份

UI 经宿主生成的 Remote 到 Host adapter，再调用 Core API；Agent native tool 直接进入 Host adapter，其他 agent 经 MCP adapter 进入同一 Core facade。官方存在 @Remote/@RemoteScope、一元/流通信和 ctx.tools 注册；没有已内置的 zodo SDK namespace。下表 zodo.* 和 zodo_* 均是本项目拟议名称，不是官方方法。[API Gateway](https://deepseek-harness.github.io/deepseek-harness/reference/api-gateway)、[工具](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/tools)

Core 配对保存安装实例及各通道能力：ui-interactive、host-agent、mcp-client 分别鉴权。Host 校验宿主连接身份与绑定，Core 再解析实际 principal/grant；模型和浏览器不能在 payload 写入 actor、role、scope、access_grant。approval_grant 是 Core 已签发批准的引用，不是调用方创造批准。SessionId/cwd、connection_generation 和 binding_id 只定位上下文，不赋予权限；Core 凭据只留 Host，浏览器不拿主 token。

UI 的数据读取、业务确认、连接/策略管理是不同能力。Host Agent grant 默认只读获准摘要和提议；它即使通过宿主工具审批，也不能调用交互批准/管理入口。Core 不信 UI 自报“用户点击过”。交互身份依赖受信组合及配对，不能证明高权限宿主代码无法仿造；同用户 shell/插件强隔离不在这个协议的承诺范围。

## 2. 操作映射

| 拟议入口 | Core 能力族 | 允许调用方 / 结果 |
| --- | --- | --- |
| zodo.query / zodo_context_query | query、planning context | UI/Agent 各按范围；分页快照及版本，不返回全部私有库 |
| zodo.capture / zodo_capture | capture.collect/clarify/convert | 有录入权限者；原文与候选分开，明确提交才转正式对象 |
| zodo.propose / zodo_goal_propose、zodo_plan_propose、zodo_review_propose | intelligence + planning/review | Core 可自主推理或接收外部 typed candidate；返回 Proposal 或 Run |
| zodo.apply / zodo_changes_commit | changes.commit | 具备提交权限和当前批准者；只返回真实 receipt，宿主允许不补足缺失批准 |
| zodo.recordProgress | execution | 获准者；计时/进展/完成为不同明确动作 |
| zodo.getRun、zodo.cancelRun / zodo_run_get、zodo_run_cancel | Run 查询/协作取消 | 只访问己方获准范围；取消受理不等于回滚 |
| zodo.watch | query/event | UI 订阅裁剪事件；Agent 需显式查询，不共享 UI 宽范围缓存 |
| zodo.approval、zodo.policy | 受信批准/管理入口 | 不注册为模型工具；UI 也必须持相应权限 |

表格不是 Core 功能上限，任务、项目、依赖、循环、容量、提醒、习惯、导入恢复等仍沿 P0/P1/P2 能力契约实施。桥不重写排程、权限或事务，只进行边界校验、超时和输出投影。

## 3. 设计示例与回执

以下是 Zodo 自有 Remote 请求包络示例，不是 Harness 原生 wire 格式，也不是可执行 JSON Schema：

~~~json
{
  "bridge_schema": "zodo.harness/1",
  "request_id": "req_21",
  "connection_generation": "bg_4",
  "binding_id": "binding_1",
  "command": {
    "schema": "zodo.command/1",
    "workspace_id": "ws_1",
    "operation": "changes.commit",
    "operation_version": 1,
    "idempotency_key": "op_21",
    "base_revision": 42,
    "policy_revision": 7,
    "payload": {
      "proposal_id": "proposal_3",
      "approval_grant": "approval_8"
    }
  }
}
~~~

Host 从配对记录取 Core 地址、实例与通道 grant，再将 request_id 传播到 Core envelope；工作空间必须与 binding/授权匹配。host_instance、session/turn/tool-call 仅作为经过验证的审计关联，不能混入 canonical command hash。native/MCP 对应相同业务规范化请求；同主体重试沿用 operation key，不以重连、fork 或 transport ID 生成新写入。

Core 成功返回原 status、operation_id、revision、data、event_cursor、replayed；Host 只包装 core_instance_id/连接 generation，UI 仅凭 committed 显示已保存。长计算返回 accepted/run_id，后续查终态；HTTP/Remote 成功、tool completed 和模型结束不代表业务提交。超时/断网发生在提交边界时先查同一 receipt，不能换新 key 重发。effects 的 pending/failed/needs_reconciliation 单独显示。

## 4. 事件与错误映射

下面是拟议 Zodo 逻辑事件，不能冒充已存在的 Harness Cordis event 或 SessionEvent：

| Core 变化 | UI 投影 | 进入 Agent 会话的内容 |
| --- | --- | --- |
| workspace.changed | 使允许范围的查询失效/拉快照 | 默认不注入；请求时给裁剪摘要 |
| proposal.changed | 待澄清/有效/过期状态 | ID、状态、允许摘要；无私密正文 |
| run.changed | 阶段、等待输入、终态 | 明确订阅且授权时给最小结果 |
| receipt.available | 更新已保存状态与对象链接 | operation ID、结果；不复制全部变更正文 |
| effect.changed | 本地保存与外部送达分开 | 必要的送达/待核对摘要 |
| policy.changed | 清缓存、重鉴权/重校验 | 撤权提示，不发送已无权访问的详情 |

事件携带 Core 实例、workspace、cursor、实体版本及允许的最小 payload。cursor 不是权限票据；watch 绑定读取主体和范围，撤权立即停止相关数据。断线补拉、重复去重、cursor 缺口 resync 与迟到 generation 丢弃均由 Client model 处理，不由 React 页面各写一套。宿主 SessionSeq 与 Core cursor 不可比较。

Core 业务 code 原样保留：REVISION_CONFLICT 重算并重批；POLICY_CHANGED 重新检查流向；PERMISSION_DENIED 不自动扩权；APPROVAL_REQUIRED/EXPIRED 打开当前内容批准；IDEMPOTENCY_MISMATCH 不盲目换键；PROVIDER_UNAVAILABLE 保留手工路径；结果未知先查 receipt。拟议 bridge code 限于 HOST_UNSUPPORTED、CORE_UNAVAILABLE、BRIDGE_BINDING_REQUIRED、BRIDGE_GENERATION_STALE、HOST_DISCLOSURE_DENIED，表示适配边界失败，不能改写成“原业务已回滚”。错误记录不可携带 token、受限标题或原始 prompt。

## 5. 日志、换模型、fork 与取消

工具的规范值、展示 meta、错误和延迟上下文都只放允许向宿主披露的内容，不能靠最终 render 隐藏泄漏。私有详情从 UI 单独授权查询，不夹带在 tool/result 中。导出来源和限制在 Core 记录，并与会话/fork 来源关联；宿主会话持久化、spill、压缩、诊断中的保留情况要实测，不承诺清聊天即删除所有副本。

每次实际模型调用核对历史披露及目的地，切换 provider、endpoint 或 adapter generation 必须重新检查。官方 llm/stream 允许拦截，但 loop 请求冻结，不能就地改 messages；失败时拒绝调用或使用不含受限历史的新会话，而非事后脱敏。[LLM seam](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/llm-streaming)

fork/replay 是宿主历史操作，不是 Core 快照恢复。历史结果仅重绘，不调用 execute；已有 Core receipt 持续有效，新变更必须基于当前事实。关闭 tab 或取消等待只结束订阅/请求；取消 durable Run 必须调用 Core cancel，检查权限并返回真实最终状态。宿主 jobs 只是可撤销的观察映射，不能成为 Core Run 的持久存储。详细反例与证据要求见 [测试矩阵](../implementation/HARNESS_TEST_MATRIX.csv)；目前没有实现或通过结果。


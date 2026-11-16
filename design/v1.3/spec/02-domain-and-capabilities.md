# 02 领域模型与内核能力

> 设计提案，尚未实现。Rust 内核管理个人事务闭环；Harness/CLI/MCP/后续 Flutter 共用能力。P0/P1/P2 为交付分级。

## 1. 领域边界与不变量

AI 理解、推理并提议；内核维护事实、规则、权限和提交。通用执行平台不在范围内。

- 意图、计划、执行事实分开；模型推测不能覆盖明确事实。
- 每个桌面 workspace 只有一个权威写入运行时；客户端不直接访问业务表。
- 任务生命周期与排程状态分开；任务完成率不能自动证明目标达成。
- 仅成功提交改变状态；通知、日历效果另行追踪。
- 相对时间解释必须绑定参考时刻和时区；可逆变化支持补偿撤销。

## 2. 聚合与关系

四个业务模块，不按实体拆服务。

| 模块 | 主要聚合及职责 | 关系与边界 |
|---|---|---|
| 工作组织 | InboxEntry、Goal、Project、Task、Note | 收集与组织；任务可独立存在 |
| 时间规划 | Availability、Commitment、CapacityBlock、Allocation、CalendarSource | 时间资源、占用和分配 |
| 执行回顾 | Habit、ActivityRecord、FocusSession、Review | 实例、配额、实际执行、回顾 |
| 智能工作流 | AgentRun、Proposal、AutomationRule | 澄清、拆解、规划、反馈、自动化 |

Workspace 保存时区、工作窗口、隐私策略和 revision。标签、收藏、排序、保存查询不另造工作类型。

InboxEntry 保留原文/来源/时间/转化引用；Goal 保留结果/标准/日期/进度；Project 保留范围/目标引用；Task 保留动作/估时/剩余量/优先级/约束/项目/父任务。

父子表达拆解，Dependency 表达先后，均禁环。汇总父项不排程；独立父级工作须声明剩余量，防止重复计算。

Note 可关联工作对象；AttachmentRef 保存位置、类型、权限、摘要/哈希，二进制不进入任务表。引用内容不具有指令权限。

## 3. 生命周期与派生状态

| 对象 | 持久化生命周期 | 独立状态/规则 |
|---|---|---|
| InboxEntry | unprocessed → needs_clarification / processed / dismissed | 可转化为多个对象 |
| Goal | active / paused / achieved / abandoned | 达成需证据/确认；可重开 |
| Project | active / paused / completed / cancelled | 不静默批量完成任务 |
| Task/Occurrence | open → in_progress → done / cancelled | 可重新打开；blocked 由依赖/手工阻塞派生 |
| FocusSession | running ↔ paused → completed / cancelled | 默认仅一个运行；持久化恢复 |
| Proposal | draft → validated → applied / rejected / stale / expired | 修改后重新校验 |

归档/删除独立标记，恢复检查关系。分配量派生 unplanned/partial/planned；截止和参考时间派生 overdue。专注时长不等于进度。

## 4. 时间、重复和习惯语义

日期、钟点、绝对时刻分别编码。绝对时刻用 UTC，保留 IANA 时区；历史不随设备时区变化。日期截止含当地整日，边界为下一当地日开始之前。改时区需选择保留钟点或时刻。

target_due 表达期望完成目标，hard_deadline 表达明确最迟期限，not_before 表达最早开始，preferred_window 表达偏好；兼容性别名 due 必须声明强度。“三点开会”形成固定预约。全天任务不占满全天；跨段分配保留总工作量。夏令时缺失/重复钟点必须校验或澄清。

RecurrenceRule 保存当地起始、时区、频率/间隔、星期/月日和结束边界。Occurrence 身份为“系列 ID + 原始发生键”，有界生成。月末缺失日期默认跳过，其他策略显式配置。

Exception 记录取消、改期和字段覆盖；改期不换发生键。编辑区分“本次 / 本次及以后 / 全系列”；后续编辑切分有效区间并保留历史。一次完成不完成整个系列。

Habit 支持固定重复与周期配额。“每周三次”不伪造成每日任务：明确周起始、时区、次数及有效完成判定。机会窗口用于规划，ActivityRecord 计数；跳过不计完成，请求去重。配额 streak 按周期达标派生。

重复依赖先支持具体实例、同周期映射；高级跨周期规则必须产生可检查的实例关系。

## 5. 占用、约束与规划稳定性

Commitment 是固定预约；CapacityBlock 保留工作容量；Allocation 将任务分钟数分到容量块或独立工作段。容量块可无任务、可容纳多项任务；任务可拆成多个分配，不强制一任务一日历事件。

全局只计算外层预约、容量块、独立分配；内部 allocation 只校验容纳和内部冲突。重叠外部事件按区间并集扣容量，保留冲突来源。导出/导入凭稳定映射去重。

硬约束：权限、有效时间、固定/锁定占用、依赖、不可用窗口、明确硬截止和不可拆分条件。软约束：期望完成目标、时段偏好、切换、连续工作、负荷和移动成本。工作窗口可配置硬/软；明确的最迟要求保留为 hard_deadline，强度不明时澄清，不静默降为软目标。

规划输入含 horizon、版本、参考时间、约束、锁定及移动预算。先保证合法，再比较截止、优先级、配额、碎片、移动成本。相同输入稳定排序，无关变化不重排全周；已开始、过去和锁定分配固定。

结果含可行部分、未安排量、软目标偏差、冲突及放宽建议。不可行保留原计划，可选局部方案；无法安排不妨碍记录任务。AI 引用真实诊断，过期日历数据须标新鲜度。

## 6. AI 工作流与提交协议

工作流：收集 → 澄清 → 拆解/依赖 → 规划 → 执行反馈 → 调整 → 回顾。外部 agent 可提交自己的提案，无需调用内置模型。AutomationRule 使用 trigger + conditions + bounded actions，必须限制权限、预算、重试并防循环。

Reasoner 的 ContextSnapshot 含引用/版本/时间/时区。本地/云端共享契约；构建请求前筛选字段/附件/provider，日志同样控制。外部读取亦为导出；不满足策略就拒绝或换模型。

Proposal 包含 ID、workspace、base_revision、actor/source、意图、typed_operations、读取引用、假设/疑问、诊断/影响、创建/过期时间、内容哈希。confidence 不赋予提交权。

校验：schema/限额 → 权限 → 对象/状态 → 图/重复 → 时间/硬约束 → 软目标/差异 → 版本/策略。低风险可自动提交；超范围需确认，绑定哈希/版本/影响。

推理/网络在事务外。提交重新检查授权和版本，单事务写业务、revision、回执、changes、effect_outbox；失败回滚。同幂等键同请求返回旧结果，不同内容报冲突。通知/日历效果异步去重重试，失败可见；原子性依据见[SQLite 官方说明](https://www.sqlite.org/atomiccommit.html)。

撤销是新补偿命令，不能假称撤回已发送通知。stale 提案需重算或显式合并，禁止覆盖新状态。

## 7. 概念存储模型

物理拆表/索引由 ADR 决定。

| 逻辑表组 | 核心数据与约束 |
|---|---|
| workspaces / policies | revision、时区、窗口、策略 |
| inbox / goals / projects / tasks | ID、状态、版本、来源、关系、归档/删除 |
| dependencies / recurrence / occurrences / exceptions | DAG、规则区间、唯一发生键、覆盖历史 |
| habits / activity_records / focus_sessions | 配额、事实、检查点、去重 |
| calendar_sources / calendar_events | provider、外部 ID、游标、权限、新鲜度 |
| commitments / capacity_blocks / allocations | 区间、锁定、任务/实例引用、容纳关系 |
| reminders / reviews / automation_rules | 意图、触发、回顾、规则/预算 |
| notes / attachment_refs / tags / saved_queries | 关联、版本、权限、引用、查询 |
| agent_runs / proposals / command_receipts | 状态、引用、diff、哈希、幂等结果 |
| changes / effect_outbox | actor、摘要、revision、效果/重试/映射 |

凭据放平台安全存储，不进入上下文。搜索索引可重建，向量库不是真相。

## 8. 内核操作与能力契约

CommandContext：actor、capabilities、workspace、request_id、idempotency_key、expected_revision。读检查范围，分页带版本；手工命令无需模型。

| 能力族 | 代表操作 | 输出 |
|---|---|---|
| capture | collect、clarify、convert、dismiss | 原文/转化引用 |
| work | create/update、move/link、complete/reopen、archive/restore | 变化/影响 |
| recurrence | expand_window、edit_instance、split_series | 实例/例外差异 |
| planning | get_context、find_capacity、propose/validate/apply、lock | 分配/诊断/版本 |
| execution | record_progress、check_in、start/pause/finish_focus | 事实/统计 |
| calendar/reminder | import_delta、export、schedule/cancel、reconcile | 本地/效果状态 |
| review/automation | build_review、confirm_actions、configure/run_rule | 回顾/执行记录 |
| query/reference | search/filter、saved_query、attach/link_note、export | 分页结果/来源 |

协议只映射契约。先用全局 revision，必要时细化读取集。

## 9. 完整能力与交付分级

| 阶段 | 交付能力 | 阶段退出条件 |
|---|---|---|
| P0 核心闭环 | Inbox、目标/项目/任务、子任务/DAG、标签/搜索、笔记附件引用、时间窗口/预约/容量/分配、基础日历循环与单次例外、busy 快照、锁定/部分计划、进度/手工专注、简单回顾、AI 工作流、提案事务、基础耐久提醒意图 | Harness插件/agent共用闭环；重启并发安全；首个发布平台通知实测 |
| P1 功能补全 | 复杂循环/完成后间隔/系列编辑、习惯/配额、日历增量导入导出、全平台提醒与专注恢复、批量编辑/保存筛选、多视图查询、丰富统计/日周回顾、有限自动化、数据导入导出/恢复 | 真实一周可用；日历/通知失败可恢复 |
| P2 深化互联 | 跨周期依赖、跨系列联合规划、弹性容量、扩展自动化、多设备同步、更多 provider、内容检索/个性化 | 独立隐私/同步验收；保留本地模式 |

Harness和后续Flutter多视图使用查询投影，不另存可写权威状态；团队协作不在当前范围。

## 10. 关键验收案例

1. UI/CLI/MCP 同目标关系一致，内外 AI 共用校验。
2. 汇总父子不重复估时；A→B→A 拒绝且原图不变。
3. 会议与容量块检查外层冲突；内部任务不重复扣容量。
4. 不可行返回缺口/原因；不移动锁定、不缩时、不覆盖原计划。
5. 无关备注不重排全周；冻结输入稳定，已开始分配不动。
6. 单次改期无重复；完成实例不完成系列；月末/DST 可预测。
7. 周配额正确计数，重复/跳过/撤销正确，streak 可重建。
8. 并发冲突明确；重试不重复创建；中断无半套计划。
9. 提醒失败独立重试；日历导出再导入不重复占用。
10. 专注重启恢复；25 分钟记录不自动完成任务。
11. 禁止导出的附件不进入 prompt、日志和错误；允许字段仍可用。
12. 回顾引用执行事实；模型不可用仍可手工管理和提交。

待 ADR：软截止权重、移动预算、日历冲突策略、同步身份/协议。实现前用示例与数据定案，不阻碍 P0 单机闭环。

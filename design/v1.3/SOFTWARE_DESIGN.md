# Zodo 软件设计方案

版本1.3 · Rust独立内核与Harness首期插件 · Flutter后续接入

# Zodo 新一代项目设计方案

版本 1.3　设计日期 2026年10月24日　状态 Harness首期适配路线已确认

## 1 项目决策与交付范围

我们将在新项目根目录独立实现 Zodo。Rust 负责可独立运行的个人事务 AI 内核；第一版交互入口采用 DeepSeek Harness 插件，Host 侧用 TypeScript/Cordis 桥接，UI 按宿主 React/Slots 与公开通信契约接入。Flutter 保留为后续独立跨平台客户端。旧项目只读；内核理解目标、拆解任务、安排时间、跟踪执行和回顾的完整目标保持不变，本地/云端模型按隐私策略接入。

本设计包采用 1.3 路线：独立 Rust Core、Harness 首期插件、MCP/CLI 通用入口及后续 Flutter。清晰日程继续提供视觉、文案与信息层级参考，现有七张图不是 Harness 新界面或已实现插件。当前交付为完整设计、计划和持续开发提示词；Host+Client 树外安装、真实模型、Rust/插件运行和目标平台均待实现验证。

产品定位为可独立运行、可被不同 agent 使用的个人事务 AI 内核。它持续维护目标、工作、时间和执行反馈，向不同界面提供一致能力。功能完整性来自领域语义与工作流覆盖；客户端负责将其组织成易用的操作体验。

## 2 用户结果与核心场景

第一类场景是从模糊目标建立可执行计划。用户说明结果、期限、可投入时间和不应移动的预约；系统保留原文，提出必要澄清，生成任务与依赖，检查容量，展示变更，再按策略提交。

第二类场景是日常收集与执行。用户可以快速收集零碎事项，也可以手工编辑，不要求每次操作都经过模型。Today 按承诺、剩余时间、阻塞和精力推荐下一步；开始专注和完成任务分别记录。

第三类场景是变化后的调整。临时会议、任务超时、某次习惯跳过或用户改变目标，都触发有范围的候选调整。系统保留已开始、过去和锁定的安排，解释未排入事项及容量缺口。

第四类场景是多入口协作。Harness Agent、插件人工 UI、CLI、MCP 和后续 Flutter 使用同一份 Zodo 事实、权限、版本、幂等和提交机制。外部 Agent 可直接提交结构化候选，Rust 校验和提交，不强制让内置模型重复理解。Harness 自己的执行计划、会话目标与 Zodo 长期个人目标分别管理，通过引用关联。

第五类场景是回顾与长期改进。回顾使用真实执行记录、用户报告和明确标记的推断，提出估时、节奏或任务粒度调整。被接受的偏好可查看、修改和撤销，不自动把一次行为升级为永久规则。

## 3 完整功能范围与产品边界

长期功能范围包括收集箱、目标、项目、里程碑、任务层级、依赖、标签、上下文、笔记与附件引用、精确过滤和全文搜索；时间窗口、预约、容量块、任务分配、循环规则与例外、习惯配额、提醒；专注会话、进度、统计、日周回顾；日历集成、有限自动化、导入导出、备份、隐私和 agent 权限，以及后续多设备同步。

目标、项目、任务和习惯无需强制嵌套。用户可以只管理几个独立待办，也可以管理跨月目标。复杂能力按需出现；首次使用优先展示收集、下一步与一份可检查的计划。

个人事务域内允许跟踪等待他人的事项和共享来源，但首期不承诺团队资源排程、多租户管理或自动对外执行。后续扩展必须复用相同授权、提案、效果记录和恢复机制。完整功能分级以第二章能力矩阵与第四章里程碑为准。

## 4 四个业务模块

工作组织管理原始收集、结果目标、项目、任务、层级、依赖和资料引用。时间规划管理可用容量、外部占用、硬软约束与候选分配。执行与回顾管理发生实例、实际投入、进度、习惯达标和回顾。智能工作流负责上下文、澄清、拆解、规划、重规划和解释。

各模块共享身份、版本、时间、来源和策略。它们在同一进程和事务边界中协作，不按每个功能拆服务。内核定义存储、模型、时钟和事件端口，外层负责实现。产品 UI、Cordis Context、React、宿主会话类型、MCP 对象、数据库对象和模型 SDK 不进入领域对象。

图示中的调用箭头表示运行关系；源代码依赖经 ports 反向注入。客户端进入统一 application facade，智能工作流生成 Proposal，确定性规则检查后由应用层提交，副作用由持久队列处理。

## 5 智能内核设计

内置流程仍为输入与事件、上下文快照、理解澄清、提案、确定性校验、批准、提交、执行反馈、重规划和回顾。Zodo 保留领域智能与可恢复工作流，不能降级为任务 CRUD 数据库。Harness 可承担交互式模型与通用 Agent 循环；Core 的 ModelGateway、内部规划/回顾与无宿主运行能力继续保留。宿主模型是可选提供路径，不成为 Core 的必需依赖。

模型网关按任务角色选择能力。输入提取、目标拆解、估时、计划解释和回顾可以使用不同模型配置。模型只提供候选；任务事实、真实时间换算、图约束、可用容量、权限和最终提交由内核确定。schema 合法不等于语义正确，语义正确也不等于获得提交授权。

上下文快照有数据版本、参考时刻、IANA 时区、选择理由和来源引用。优先使用结构化事实与全文检索；向量检索在有明确需求和评测后作为候选召回扩展。保留结构化理由和操作轨迹，不要求保存隐藏思维过程。

实时编辑预览使用可取消短请求，不逐键生成耐久 Run。多步骤计划、导入和回顾使用耐久 Run，断线后可查询。已提交变更的撤销是新的补偿操作，不能用取消状态掩盖已发生的事实。

## 6 隐私与执行位置

隐私策略覆盖模型、外部 agent 读取、连接器、诊断日志和附件处理。构建 prompt 或传输响应前先选择允许字段；禁止先发出再脱敏。local_only 禁止云模型回退，模型不可用时保留手工操作和明确错误。

策略同时表达数据分级与可信执行边界。device_only 表示不得离开当前设备，不能因目的地是用户自托管服务而自动允许传输；trusted_workspace 表示允许在用户明确授予的工作空间运行；cloud_allowed 允许指定 provider 接收最小必要内容。远程 Web 客户端在发送材料前展示其运行位置，不能把远程计算标为设备本地。

默认 Zodo 日志记录 ID、阶段、耗时、状态和版本，原文诊断需主动启用。此策略不会改变 Harness 自身会话记录：交给 Agent 的数据可能进入日志并随后续模型调用重用，必须在进入会话前最小化并检查会话授权。人工 UI 视图与 Agent 视图分开。RPC token 不能保护同一系统身份下的高权限宿主或任意 Shell；强隔离需要独立系统身份/沙箱及单独验收。

## 7 关键产品语义

API 的 target_due 表达期望完成目标，hard_deadline 表达明确不可违背的期限；兼容性别名 due 必须附带强度，不允许含义模糊。界面分别展示期望完成和必须完成。自然语言明确要求的最迟期限映射为硬截止；无法判断强度时保留疑问。硬截止无法满足时任务仍可记录，但计划必须报告不可行或风险，不能伪造可行结果。

工作窗口和休息可配置为硬约束或偏好。用户允许放宽时生成明确变更，调度器不能自行越过硬边界。固定预约、容量块和任务分配分层计费；重复事件按唯一发生键记录例外；汇总父任务不重复计入工作量。

P0 支持基础日历循环和单次例外、基础耐久提醒意图、外部 busy 快照、执行反馈和简单回顾。P1 完善复杂循环、周期习惯配额、日历双向增量同步、多平台通知恢复和丰富统计。数据模型从一开始保留这些完整语义，交付可分阶段。

## 8 跨平台产品承诺

第一阶段先验证选定宿主版本上的 Harness 插件与独立 Rust daemon。插件可以连接/辅助启动 Runtime，但进程、数据、耐久 Runs 与提醒不能依赖会话页生命周期。Windows/macOS/Linux 的支持按插件、Rust 二进制及依赖交集验收；Harness 网页的手机访问不等于原生 Android/iOS 已完成。后续 Flutter 桌面连接 daemon，移动端嵌入 Rust，Flutter Web 连接认证 API。

Web 初版离线仅保留缓存与未提交草稿，不宣称浏览器内核、本地推理或多副本合并。原生移动端可以离线管理事务，但完整 AI 能力取决于已安装模型和设备条件。系统挂起或终止后的持续工作、通知和恢复须分别验证。桌面后台行为由用户控制，不随每个 UI 页面生命周期重新初始化。

支持声明必须附平台、架构、工具链、构建、安装、运行和已知限制。Windows 本机不能代替 Apple 真机或签名验证；Flutter 支持列表也不能代替 Rust、模型运行时和产品依赖的交集验证。

## 9 非功能预算与质量目标

以下是待基准验证的产品预算，不是已有测量结果。P0 固定一台参考机器、release 构建、数据种子和负载后报告实际值，未达预算需要分析或调整设计。

| 项目 | 初始预算或约束 | 验证方式 |
|---|---|---|
| 常规交互 | UI 输入和导航不等待推理或磁盘同步 | Harness 输入与渲染延迟；后续 Flutter 帧记录 |
| 结构化查询 | 10000 条历史任务下常规分页暖查询 P95 100ms 内 | release 内核固定基准 |
| 单次提交 | 无外部网络的典型小变更 P95 200ms 内 | 事务耗时与故障注入 |
| 规划计算 | 30 日有界窗口与 500 个候选分配的首轮候选以 1s 为初始预算 | 冻结输入和独立约束验证 |
| 模型延迟 | 按模型角色、设备与 provider 分别建立预算 | 首结果、总耗时、取消、成本与内存 |
| 数据一致性 | 崩溃、重试和并发下无半提交与静默覆盖 | 事务、receipt、outbox 对照 |
| 可达性 | 键盘、触控、读屏、缩放与减少动效具备完整核心路径 | 桌面和手机真实操作 |

测试集优先冻结。保留旧七字段解析作为独立基线；新增多轮理解、拆解、依赖、容量、重规划、执行反馈和隐私场景。开发回归与独立验收分开，不能使用模型自评分或宽松 JSON 清洗替代质量证据。

## 10 新项目实施与完成定义

先完成 Harness 树外 Host+Client 插件的构建/安装/加载、Core API 连接、版本协商及权限最小 PoC，再实现 Rust 无 UI 纵切与 Harness 真实闭环。MCP、受控模型、P0/P1/P2、后续 Flutter 和全平台验证依 implementation/PLAN.md 的 A0–A7 连续推进；原型或 P0 通过不代表全部目标完成。

最早技术纵切覆盖宿主候选→Rust校验/提案→UI差异→当前批准→提交→执行反馈，并验证重试、旧批准、跨工作空间、热重载、Host退出和Core重启恢复。至少两类客户端访问同一Runtime；关闭宿主后CLI仍能读取事实，背景任务按已验证部署策略运行。宿主SDK版本、分发与设备限制保留实际证据。

旧数据经只读快照、中立导出、dry run、字段及数量核对后导入新库。新项目不默认读取设备上的真实私密数据，不修改旧源库，不将旧内存排程当作已承诺日程。依赖锁定、许可证、构建脚本、测试和交付证据随新仓库维护；建立远程仓库与发布单独记录。

完成定义包括真实客户端操作、内核不变量、实际模型路由、平台安装运行、恢复、隐私和迁移的证据；模拟交互通过只说明原型可用。全部功能范围、阶段退出条件和需求验收见后续章节。

## 11 官方依据与事实边界

本设计基于当前旧项目源码审计及本轮核验的官方资料。主要依据包括 [MCP 2026 规范](https://modelcontextprotocol.io/specification/2026-07-28)、[官方 Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)、[Flutter 原生互操作](https://docs.flutter.dev/platform-integration/bind-native-code)、[Dart Hooks](https://dart.dev/tools/hooks)、[SQLite 事务](https://sqlite.org/lang_transaction.html)。具体平台与协议边界见第三章。

竞品功能仅作为成熟能力基线。Todoist 已提供 AI agents 集成；Reclaim 2.0 已将工作时段与任务推荐分开。Zodo 的独立运行、统一事务和数据控制属于待实现的组合定位，不宣称单项功能独有。参考链接与比较边界见第四章。


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


# UI产品规范与交付计划

版本1.3 · 2026年10月24日。首期界面是DeepSeek Harness内的Zodo插件，使用宿主原生Host扩展及React/Slots；Rust独立内核范围不变，Flutter后续提供独立客户端。以下为设计与验收要求，尚未实现。

## 1. 业务目的地与宿主布局

逻辑目的地仍为今天、收集箱、目标/项目、日程、回顾/习惯，辅以任务、搜索、专注、提案、设置、权限和恢复。30个屏幕族与16弹层保持稳定编号。五业务入口在Zodo工作台内部组织为宿主允许的标签、面板或路由，不夺取或替换Harness自身全局导航。

人工操作经插件Client model、公开Host Remote/RPC和受控Core连接进入统一应用层。React/Cordis只拥有导航、草稿、焦点和查询投影，不能复制领域约束、权限判断或直接写SQLite。首期可借用宿主对话入口，同时保留独立的结构化编辑和比较视图，不要求所有动作都由模型推理。

后续Flutter使用KernelClient复用契约和业务目的地，原生移动采用自己的五入口与详情路由。Harness网页在手机显示不代表原生客户端、设备离线写入、通知和后台推理已验证。

## 2. 视觉参考与组件

清晰日程七张现有PNG提供颜色、空间、层级和交互意图参考，属于旧独立布局，不是Harness插件截图；M01仅作后续窄屏/手机参考。首期需按宿主实际Slots/面板约束重新映射，不在本轮声称新稿或可点击原型已完成。tokens仅作用于Zodo插件，适配宿主主题，不全局注入覆盖宿主样式。

核心组件包括IntentComposer、TaskRow/Detail、Goal/Project层级、Agenda/Timeline、ProposalDiff、ExecutionRecord、Review及ModelPrivacy/Grant。生成期间输入可编辑，中文IME组词不触发推理，旧请求不覆盖新输入。操作键盘/触控可达，焦点、读屏、文字放大和减少动效按实际载体验证。

## 3. 跨页面闭环

完整八条核心流程及辅助路径见ui/01-screen-and-flow-spec.md：收集与理解、目标拆解、稳定规划、执行反馈、循环例外、回顾和外部Agent协作都必须有明确入口、当前状态、退出与恢复。计划比较展示增删改、硬/软约束、未排入及影响；批准绑定当前提案内容和数据版本，宿主工具执行许可不代替业务批准。

专注结束、任务完成与目标达成分别记录。模型失败仍可手工管理；取消生成不撤销已提交事实。Core提交成功后才显示已应用；日历/提醒等外部效果独立跟踪。保存结果未知先查同一操作回执，不盲发新写入。

## 4. 状态与宿主生命周期

| 状态 | 界面与恢复 |
| --- | --- |
| 空/加载/生成 | 保留输入与已有内容，阶段可见；明确取消和手工入口 |
| 失败/离线 | 保留草稿，区分浏览器断线、Host不可达、Core不可达、模型失败；未提交不假成功 |
| 冲突/过期 | 拉取最新事实，说明依据变化，重算并重新批准，不覆盖新状态 |
| 重复/未知结果 | 复用原操作身份和回执，不能因重连制造第二次业务修改 |
| 热重载/关闭宿主 | 清理插件订阅而非领域事实；重新连接后以Core快照/Run/游标恢复 |
| 分叉/回放 | 只恢复引用与显示；不把会话回退当作业务撤销，不重新执行外部效果 |
| 无权/撤权/模型切换 | UI与Agent视图分开裁剪；旧授权和旧批准不沿用；会话已有内容不能假装被追溯收回 |

## 5. 隐私与运行位置

展示浏览器、Harness Host、Core和实际模型的运行位置及数据流向。工具结果进入Agent会话前最小化，不能因当前选中本地模型就把device_only材料写入以后可能切云的会话。人工UI显示较完整资料并不自动赋予Agent读取许可。宿主及其插件处于明确的信任边界，高权限宿主的强隔离需要独立部署和单独验收。

## 6. 分期与退出标准

| 阶段 | 产出与验收 |
| --- | --- |
| A0 | 锁定宿主/SDK/构建辅助，树外Host+Client能构建、安装、加载、调用Core并协商版本 |
| A1–A3 | Rust无UI纵切、Harness真实闭环、受控模型、MCP/CLI与完整P0；覆盖重启、并发、过期、撤权和真实请求 |
| A4 | P1循环/习惯/查询/统计/导入恢复/增量集成等，按有限可验收定义持续推进 |
| A5 | 后续Flutter独立跨平台客户端、FFI、原生通知/恢复和平台实测 |
| A6–A7 | P2完整范围、同步/个性化/互联及全部能力与平台回归；未验收项不能标全完 |

详细顺序和持续工作规则以implementation/PLAN.md为准。阶段完成是进入下一阶段的条件，不是结束完整目标。mock、静态图、跳过的测试和宽松schema清洗均不能充当真实功能证据。

## 7. 验收与旧项目迁移

业务不变量、事务失败、版本/幂等、时间/DST/循环、模型质量与数据流、宿主安装/卸载/重载、两类客户端、实际平台安装/通知/恢复分别验证。每项记录源码/版本、命令、退出码、原始证据和缺口；自测修改与产品修改对应，失败先定位修复再回归。

旧项目只读参考，不复制providers/HiveObject作为新边界。迁移使用独立快照、中立导出、预检、映射、去重、数量/指纹核对与可恢复导入，不读取或改写旧私密库。原字段完整度不能作为模型可靠性或自动批准依据。

## 8. 竞品与交付边界

传统事务能力、AI工作流和完整P0/P1/P2范围沿用领域规范。更换首期宿主不删除任务/项目/搜索/循环/提醒/专注/习惯/回顾/备份等能力。未具体定义的更多provider或高级自动化先收敛为有限用例和验收，记录ADR，不以模糊目标无限扩大范围或自行删项。


# 架构决策与实施边界

版本1.3 · 2026年10月24日。用户已确认Harness首期插件路线，Rust个人事务AI内核完整目标不变，Flutter后续接入。技术实现及平台兼容尚待验证。

## 1. ADR 001 Rust模块化单体

保留domain/application/intelligence/ports与统一事务边界。zodo-kernel与zodo-runtime是首期核心，zodo-ffi是后续客户端适配。Core不依赖Cordis、React、Flutter、MCP或特定模型SDK；不要复刻通用Harness插件框架。接受Rust跨平台与嵌入成本，以纵切和真实分发验证。

## 2. ADR 002 首期适配器与后续Flutter

更新原Flutter优先顺序：首期采用Harness的TypeScript/Cordis Host扩展和React/Slots UI，通过统一Core API与事件桥接。后续Flutter仍在完整范围，KernelClient复用同一业务语义。共享schema由Zodo拥有；宿主Remote生成物只定义宿主边界。原先首期就制作完整Flutter产品的顺序由本决策替代。

## 3. ADR 003 SQLite与短事务

Zodo保存权威事实、版本、Proposal/Run、receipt与outbox，默认每workspace单写者。推理和外部网络不在SQL事务内。宿主Session保存会话与业务引用，不作为个人事务唯一数据库。外部效果最终完成或待核对，不能承诺跨系统原子提交。

## 4. ADR 004 共享提案与提交

人工UI、内置AI、Harness Agent、CLI和MCP共用应用校验。简单明确修改可直接形成变更，复杂规划使用提案。预览不改正式事实；批准绑定内容、版本、范围和有效期。宿主批准运行工具不是Core批准业务变更，模型自报approved或actor无权提升许可。

## 5. ADR 005 保留领域智能与ModelGateway

Core保留目标/任务/时间领域智能、耐久工作流、重规划、执行反馈和回顾。Harness可承担交互式通用Agent和模型调用，外部明确结构化候选不重复交给内部模型理解。Core ModelGateway仍可独立接本地/云端提供方，关闭宿主后不丧失原有内核目标；所有路由与降级受隐私、能力和预算约束。

## 6. ADR 006 MCP通用入口

Harness首期优先原生tools与UI桥接；MCP是Zodo面向其他Agent的独立适配器，CLI/HTTP/MCP共用应用层。不要为同一宿主重复暴露两套相同工具。SDK版本及协议兼容在实现时核对锁定，不把设计示例当完整schema或实际兼容证明。

## 7. ADR 007 时间与稳定性

target_due、hard_deadline、not_before、preferred_window各自明确；固定预约、容量块、内部任务分配不混计。过去、已开始和锁定安排不自动移动是P0不变量；后续优化重排权重不能推迟该约束。无解如实报告，原计划保留。

## 8. ADR 008 多端一致性

每个选定workspace/runtime有明确权威写者，不能把多个离线数据库静默合并。首期宿主与CLI等连接同一实例；后续移动嵌入与多设备同步分别设计身份、数据流、端到端加密和冲突处理。Harness手机网页不是原生移动完成证据。

## 9. ADR 009 清晰日程视觉参考

用户已选择第一套清晰日程。保留浅色、青绿色、列表/议程与可检查提案的层级。现有六桌面一手机PNG仅为旧独立布局参考，首期在宿主插件内部映射业务入口和状态，不覆盖宿主全局导航或主题。不得将这些图算为Harness新截图或已实现页面。

## 10. ADR 010 完整范围与持续推进

P0基础循环/单次例外、耐久提醒、busy、反馈/回顾及稳定性保留。P1复杂循环/系列编辑、配额、查询、统计、增量日历、导入恢复等继续实现。P2跨周期/跨系列规划、多设备同步、个性化及扩展互联按有限用例定义验收。Flutter后续仍在总目标，插件P0或MVP通过不结束持续开发。

## 11. ADR 011 Harness首期路线

背景是优先在Agent工作流中验证独立个人事务内核。选择薄Host/Client适配器，复用宿主模型、对话和交互能力，保持Zodo权威状态及通用契约。相比Flutter首发减少通用Agent基础设施重复搭建，但增加TypeScript、SDK兼容、二进制分发和生命周期成本。A0必须验证树外插件构建/安装/加载、公开通信与Core连接；若只能依赖大规模私有修改或强迫迁移业务到宿主，记录差异并重新评估载体，不改变Core目标。

## 12. ADR 012 授权、日志与生命周期

人工视图、Agent视图、Host会话和Zodo业务操作分别绑定身份与范围。进入Agent的数据先按可保存/可外发边界过滤，再做实际路由检查；不能假装后续换模型前能收回已记录资料。默认宿主及已安装插件处于用户信任边界；对高权限宿主的强隔离需独立系统身份/沙箱和单独验收。插件卸载只释放自身订阅，持久任务/批准/receipt由Core掌握；fork/replay不重放业务写入。

## 13. 待定技术项与定案条件

| 项目 | 定案方式 |
| --- | --- |
| Host版本与树外Client打包 | A0记录发布包/源码辅助来源、公开契约、安装和真实加载结果 |
| UI插槽与批准载体 | 实际验证工作台/详情/差异和人工身份，不虚构可用SDK方法 |
| Core分发与后台生命周期 | 锁定平台/二进制/协议，验证安装、重载、退出、重启与后台任务 |
| 模型默认值与会话数据流 | 角色评测、资源实测及本地/云/换模型/日志验证后选择 |
| 规划权重与移动预算 | 冻结规划样本与实际反馈，保留硬约束 |
| Flutter FFI与Apple分发 | A5按真实工具链、签名、设备和后台限制验收 |
| 日历双向冲突与同步 | 独立provider、身份和故障对账测试，不用mock代替外部兼容 |

## 14. 当前交付状态

本轮更新方案、计划、UI适配说明、桥接契约和持续开发提示词，没有实现产品。文字规格、旧视觉参考、Harness新UI、可交互、真实内核、平台实测分层记录；新项目依据覆盖账本连续推进。最新用户指示优先，版本1.2保留为历史。


# 06 · Harness 首期适配与 UI

状态：1.3 设计基线，2026-10-24，尚未实现。本章仅改变首个客户端：先交付 DeepSeek Harness 插件，Flutter 保留为后续独立客户端。Rust 个人事务 AI 内核及 P0/P1/P2 范围不变，不因嵌入聊天宿主而缩成任务 CRUD。以下“官方事实”来自本轮主源核对，其余结构、名称和流程均为拟议设计，必须通过 PoC。

## 1. 宿主事实与接入范围

官方将 Harness 标为快速迭代的开发者预览。其应用由 Cordis 插件组合；TypeScript 插件通过 apply、依赖注入及受生命周期管理的注册接入。Web Client 使用 Client model、React 与 Slots；Typert Remote 提供生成的一元/流式 Host 通信。这说明存在适配路径，不表示 Zodo 已兼容某个发行包。[官方 README](https://github.com/deepseek-ai/deepseek-harness)、[架构](https://deepseek-harness.github.io/deepseek-harness/reference/)、[首个插件](https://deepseek-harness.github.io/deepseek-harness/develop/basic/)

首个入口是宿主中的 Zodo 工作台，复用“清晰日程”的内容层级、青绿强调和议程比较，不替换宿主整个应用。工作台内部仍有今天、收集箱、目标/项目、日程、回顾及设置，完整屏幕/状态规格继续有效。会话工具卡只展示建议摘要与“打开 Zodo 预览”，不能把完整业务 UI 压成聊天气泡。

官方 Sidebar 提供会话旁的可停靠 tab；Slots 定义类型化组合及作用域。拟议工作台使用独立 Zodo tab，在可用扩展位置贡献打开入口，窄面板用单栏，扩大面板后用列表与详情。tab 属于某个宿主会话的布局，不表示 Core 工作空间随该会话销毁。准确 slot key、tab 注册和根入口由锁定版本的 catalog 验证，不臆造 SDK 的 registerPage 方法。[Sidebar](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/sidebar-right)、[Slots](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/slots)

## 2. 组件与独立智能内核

Rust 规划为 zodo-kernel、zodo-runtime、zodo-ffi 三个 crate；首期实现前两者，FFI 供后续 Flutter 嵌入验证，插件不调用它。Core 的 application/intelligence 保留理解、澄清、目标拆解、依赖、容量规划、执行反馈和回顾；ModelGateway 自主管理本地/云端路由、预算和取消。结构化提案经确定性校验、授权与短事务提交，Run、receipt、outbox 由 Core 持久化。

拟议 TS 插件分 Host adapter、Client model、React presentation 三层。Host adapter 负责 Core 发现/配对、工作空间绑定、版本握手、Remote、工具注册与脱敏；Client model 维护 Core 快照、事件游标和连接状态；组件只收数据、callback 和 observable 投影，不接 Cordis Context、Core 凭据或数据库对象。官方 Web Client 的分层方向支持这种拆分；Zodo 事实仍由外部 Core 持有，不能复制一份宿主业务数据库。[Web Client](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/web-client)

保留两条 AI 路径：用户在 Zodo 面板请求 Core 编排，或由宿主 Agent 通过工具提议操作。前者直接使用 Core ModelGateway，不强制借用宿主聊天模型；后者可提交已结构化的候选，无须内部模型再理解一次。两条路径都支持多轮澄清、耐久计划与回顾，而不是只做标题提取。宿主选模型不会悄悄改变 Core 的模型设置。

## 3. API、Remote 与事实同步

浏览器 → 宿主生成式 Remote → Zodo Host adapter → Core loopback API。浏览器不直接持有 Core bearer token；宿主也不把远程连接参数交给模型选择。Core 地址、实例、数据目录和执行位置由受信配置解析，禁止任意 URL/路径穿透，避免把桥变成网络或文件代理。

官方 @Remote/@RemoteScope 和 stream 描述符生成 Host/Client 约定；拟议 Zodo namespace 必须经锁版工具链生成，不能只写 ctx.remote.zodo 并假定它存在。Remote 只封装现有 Core Command/Query/Result/Error/Event，执行路径见 [桥接契约](contracts/HARNESS_BRIDGE.md)。仅做格式、大小、连接和绑定检查，领域/权限/时间规则在 Core 统一执行。[API Gateway](https://deepseek-harness.github.io/deepseek-harness/reference/api-gateway)

订阅使用 Core cursor 与实例身份，不能拿 Harness SessionSeq 当业务版本。重连先鉴权、拉基线再补事件；重复事件去重，游标失效拉新快照，切工作空间丢弃旧连接 generation 的结果。仅按当前权限投影事件，不能经订阅泄露不可读标题。Agent 得到裁剪摘要，UI 独立读取获准详情；两者不共用宽权限缓存。

## 4. 人、Agent 与批准的边界

初次连接由 Core 的受信本地入口配对。宿主认证的交互 UI 与 native tool/MCP 调用使用不同 Core grant，绑定安装实例、调用通道、workspace、能力和有效期。SessionId、cwd、项目名仅为定位/审计线索，不是授权。浏览器和工具参数均不能自报 actor、管理员身份或访问 grant；实际主体由受信传输确定，Core 再校验。UI 仍只能在被授予的工作空间操作，策略管理必须是单独权限，不能凭“来自界面”自动提权。

默认 Agent 可读指定目标的必要字段并提议，不能修改规则、扩大范围或批准自己的建议。业务批准由 Core 签发，绑定主体、内容哈希、数据/策略版本、effects、有效期和单次消费；仅受信交互批准入口或已存在的有限预授权可以产生它。批准引用可以随提交传递，但“approved:true”、模型自称用户及宿主提示不替代验证。

官方宿主审批 seam 判断一个具体宿主操作是否允许继续，存在一次性允许和拒绝等结果。Zodo 把它作为前置工具门：宿主拒绝则不调 Core；宿主允许仍可能被 Core 拒绝，不能把宿主批准 ID 转成业务批准。预览后内容变化须重批，过期计划不能复用原按钮。[宿主审批](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/approval)

## 5. 会话历史、模型切换与数据流向

官方会话日志包含用户/助手消息、工具调用参数与结果；持久化 seam 可保存事件日志，不能假定只保存在聊天 UI 内存。插件必须在生成工具参数、规范返回值、展示 meta、附件、错误及延迟上下文之前最小化，不把私密正文藏进“模型看不到”的字段后声称未泄露。默认 Agent 工具只返回允许外发的必要摘要；device_only 内容只在同一可信设备上的 Zodo 私有面板/Core 工作流处理，不进入宿主会话。[会话](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/session)、[持久化](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/persistence)、[工具契约](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/tools)

“Zodo 建议：此设备处理”与“宿主聊天：所选服务”分别展示。local_only 约束 Core 模型路由，device_only 约束内容不得离开其归属设备；Core 与宿主同机、浏览器远程访问、自托管服务器、宿主云模型是不同流向，不能统称本地。远程浏览器也不自动有权读取 device_only 数据。用户直接把秘密粘贴到宿主聊天、其他高权限工具读取文件，均超出桥接内容过滤的保护范围。

若允许某份摘要进入特定本地/云路由，Core 记录该导出的分级、范围、允许目的地与会话来源。每次宿主模型请求，包含重试、恢复、fork、压缩/外置内容重建和换模型，都要核对历史中相关披露限制以及实际 provider/endpoint/adapter generation。旧资料不因换模型、改会话名或回放而解除限制；不明来源/目的地默认拒绝继续发送，提示建立不带受限资料的新会话。

官方 llm/stream 可包围模型调用，但 loop 构造的请求是冻结的，不能就地修改 messages。拟议 guard 在分发前拒绝不合规请求，不承诺靠临出网改写历史脱敏。必须证明受支持组合中的模型、标题/压缩等辅助请求也经过相应检查；覆盖不全时不开放受限资料向宿主导出。这不是对恶意插件不可绕过的监控器。[LLM 调用 seam](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/llm-streaming)

## 6. 威胁模型与凭据

Core 拒绝越权 RPC 能防范正常组合中的参数伪造、误调用、越工作空间访问和旧批准重放。它不能抵御拥有同一 OS 用户权限的恶意宿主插件、任意 Node 代码或 shell 直接读改数据库、窃取进程凭据、替换桥或另开出网连接。RPC token、Origin 检查和 UI/Agent 双通道均不是同用户代码的强隔离。宿主官方也明确插件/命令权限与沙箱边界不保证隔离。[官方安全说明](https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.zh.md)

首期信任安装的宿主组合与 Zodo 插件，使用受控的构造数据完成验证；Core 数据置于宿主项目目录外，只授必要 OS 权限。若要防不可信宿主，应采用独立 OS 身份/进程权限或远端可信 Core 的最小 RPC 边界，并分别验证文件、网络、凭据访问；这仍不阻止宿主外发已经获准读到的内容。未做到就不能宣称强隐私隔离，不能以界面锁图标代替这一限制。

Core/provider 密钥存 Core 安全存储；Host 只持最小连接 grant，不复制全部模型密钥进 Harness 设置。浏览器只用短期受限会话，日志记录 ID、状态、耗时与路由摘要，不记正文、token 或完整 RPC。诊断包按权限裁剪，安装包与会话历史均不携带真实凭据。

## 7. daemon、会话与重放

每数据目录一个权威 Core，由文件锁、实例身份和协议握手防重复。插件持有连接租约，不拥有数据库生命周期；热重载/卸载清理 Slots、工具、Remote、订阅及临时请求，不删除事实或杀掉其他客户端使用的 Core。官方 ctx.effect 可清理自有资源，但外部进程、socket 与未完成异步动作仍须显式处置并验证。[插件生命周期](https://deepseek-harness.github.io/deepseek-harness/develop/basic/)

持久模式采用独立的用户级 Core 管理器；宿主退出后耐久 Run 与提醒按权限继续或等待输入。开发模式若 Core 随启动器结束，明确显示“宿主退出后暂停”，重开按 lease/checkpoint 恢复，不能承诺常驻。尚未实现的 OS 后台保活不是首期已完成能力。宿主 owner 结束可能清理其 jobs，因此 Harness job 只做观察映射，Core Run 不依附于其存活；Host 停止等待不等于取消 Core 工作。[宿主 jobs](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/jobs)

官方 fork 克隆所选会话事件前缀；Zodo workspace 及已提交事实不随会话 fork、删日志或 replay 回滚。回放只渲染已记录的摘要/回执，不执行历史写入；再次执行必须是当前新意图，重新检查权限、版本与批准。网络重试沿用原 operation/idempotency，fork 也不能用新 session ID 绕过去重。Core 已提交但宿主日志丢失时查询 receipt，绝不把“聊天看不到”当作未发生。[会话 fork](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/session)

## 8. native tools 与 MCP

首个宿主使用 native tools 封装同一 application facade，保留 MCP stdio/HTTP 作为其他 agent 的通用入口，二者共享主体/范围检查、Proposal、receipt 和事实。模型能调用的 schema 只包含业务输入，不暴露 UI 批准、策略管理和 Core 地址设置。官方工具注册与输出契约只解决宿主侧分派，不承担 Zodo 校验。

同一 Harness 会话默认只启用一份 Zodo 工具目录，避免 native 与 MCP 同名双注册；跨客户端并行访问仍由 Core 版本、幂等和单写者保证。MCP 可选 Tasks 不替代业务 Run。没有 Harness 时，CLI 必须能独立走通理解/澄清/规划/提交/反馈/回顾，并按相同策略使用本地或云模型。

## 9. 分发与 PoC 出口

插件使用可安装 bundle，预构建 Host 与 Client 产物并声明宿主兼容范围。官方树外浏览器半侧要求 dsh.client、./client 导出与 lazy-CJS factory；仓库内 clientBundle 预设不在已发布包，首期要把等效构建步骤纳入自身发布流程，不能依赖用户机器旁边有 Harness monorepo。生成 Remote 贡献和 peer 实例共享也必须在树外安装验证。已发布 SDK 能力直接锁包使用；构建辅助可按公开运行时契约自行实现，或按许可固定源码版本与输入产物；其余均记为未验证假设，不承诺开箱可装。[客户端模块](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/client-modules)、[设置页构建示例](https://deepseek-harness.github.io/deepseek-harness/reference/cookbook/adding-a-settings-card)

锁定测试宿主发行/commit、Node、包管理器、插件产物、Core API/二进制构建和 schema；详细值进入实现锁文件，不在正文堆候选版本。优先 tarball/注册表预构建发布；Rust 二进制按 OS/arch 分包，校验 SHA256/签名与协议范围，不运行未声明的联网安装脚本或自动编译。版本不匹配拒绝激活，不自动迁移后再回退旧二进制。[打包安装](https://deepseek-harness.github.io/deepseek-harness/develop/basic/publish)

A0 硬门槛是“树外 Host+Client 插件在锁定版本下可构建、安装、加载及调用”，不因官方某预设未发包就判不可用。用最小 Rust 测试进程验证干净 profile 安装、进程发现/握手、React 表面、typed Remote、原生工具往返、异步取消与重载/卸载/退出的连接清理；构造数据只证明桥接，不冒充业务内核。首期不 fork 宿主业务或主源码；PoC 如确需修改，记录差异、兼容成本与 ADR 后再决定。

A1 建立真实 Core 后，A2 完成宿主结构化提议、React 差异、用户业务批准、提交与执行反馈；A3 补齐 Core 自主 AI 编排、独立 ModelGateway、native/MCP 共用事实，以及耐久 Run、隐私切换、重放、重启与跨工作空间等完整 P0 验收。A0 的连接烟测不能替代这些业务断言，也不要求先实现 A3 才能通过 A0。

验收使用 [Harness 测试矩阵](implementation/HARNESS_TEST_MATRIX.csv)，按各行阶段执行，所有测试目前未执行。找不到已发布扩展能力时记录缺口，不能用手改宿主内核或只有源码 monorepo 可运行冒充插件分发完成。Core 各阶段范围及后续 Flutter 客户端继续保留，未具备平台与模型能力逐项标未验证。

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

官方 @Remote/@RemoteScope 和 stream 描述符生成 Host/Client 约定；拟议 Zodo namespace 必须经锁版工具链生成，不能只写 ctx.remote.zodo 并假定它存在。Remote 只封装现有 Core Command/Query/Result/Error/Event，执行路径见 [桥接契约](../contracts/HARNESS_BRIDGE.md)。仅做格式、大小、连接和绑定检查，领域/权限/时间规则在 Core 统一执行。[API Gateway](https://deepseek-harness.github.io/deepseek-harness/reference/api-gateway)

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

验收使用 [Harness 测试矩阵](../implementation/HARNESS_TEST_MATRIX.csv)，按各行阶段执行，所有测试目前未执行。找不到已发布扩展能力时记录缺口，不能用手改宿主内核或只有源码 monorepo 可运行冒充插件分发完成。Core 各阶段范围及后续 Flutter 客户端继续保留，未具备平台与模型能力逐项标未验证。


# 清晰日程视觉系统与 Harness 组件

版本 1.3 · 2026-10-24 · 设计规格，插件界面尚未绘制或实现。

用户保留第一套清晰日程的信息层级：平静列表/议程、轻分隔、清楚主动作和可读差异。七张原 PNG 是此前独立客户端的静态视觉参考，未改造成 Harness 截图，不能证明插件已经出图、装载或可交互。D01–D06 参考工作台内容，M01 仅参考后续窄屏/手机的逐项确认；不逐像素搬运整套导航、外壳、底色或尺寸。

## 宿主框架与局部样式

首期使用 React，通过宿主 Slots 提供 Zodo 工作台、详情和批准视图，经 Host RPC 桥接 Rust Core。Harness 保有全局导航、会话、右侧停靠布局、主题、快捷键、Esc/模态和插件生命周期的所有权。五个业务入口“今天/收集箱/目标/日程/回顾”仅在 Zodo 自有内容区组织，可为内部分段或局部导航，不要求替换宿主 sidebar 或 root。

组件继承可用的宿主字体、主题、焦点、控件与模态约定；清晰日程的青绿强调仅作用于 Zodo 局部并服从对比度。样式以插件根节点作用域限定，不全局 reset、不覆盖宿主 CSS、不注册第二棵 React root；不得为私有组件导入其他功能插件实现。tokens 见 [design-tokens.json](../prototype/design-tokens.json)，具体宿主局部样式约束以更新后的 tokens 和技术验证为准。

Slots 提供类型化组合与生命周期注册；具体扩展位、props、可用容器需在锁定 Harness 版本后验证，不把本文的概念组件名当已存在的宿主 API。[官方 Slots](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/slots)

## 容器适配与信息层级

按插件实际可用面宽适配，而非浏览器外窗宽：宽停靠面可显示主区与详情/提案双栏；受限侧栏优先摘要、下一步和“打开详情”；窄容器用单栏和可返回详情。既有三栏 PNG 可参考内容分组，不能照搬其左侧全局栏。200% 文字、宿主分栏/浮出/收起、长正文和 IME 键盘下主动作仍可达。

Today 先显示承诺与下一步，Inbox 保留原文，目标显示标准和任务关系，计划显示差异与未应用状态，回顾分开事实/反馈/建议。完整差异不能被挤进一条聊天气泡；空间不足时打开宿主允许的详情面，不删掉冲突和未排入事项。右侧 Sidebar 可承载按地址打开的产品页面，具体 tab/地址注册以 PoC 为准。[官方 Sidebar](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/sidebar-right)

后续独立 Flutter 客户端才采用原桌面/平板/手机布局与手机五底栏；M01 不代表 Harness 已拥有手机应用、原生文件系统、后台推理或系统通知能力。

## 组件契约

- ZodoWorkbench：插件内五入口、局部搜索/收集、Core workspace 和连接状态；保留筛选/滚动，不修改宿主全局导航。
- IntentComposer：输入始终可编辑、IME、防抖、最新请求、生成/取消、实际处理位置与手工替代；预览不写正式事实，也不默认送入宿主 agent 会话。
- TaskRow/Detail：完成、计划、阻塞、剩余量分开；React 只持有投影和草稿，Core 校验关系/时间/提交。
- ProposalDiff/Approval：逐项差异、来源、假设、影响、stale/冲突与确认；人工批准经独立可信 UI face，绑定当前内容，不把 agent 工具结果或宿主工具允许一次当 Core 批准。
- Agenda/Timeline：预约、容量、分配与建议有文字/形状区别，锁定和未排入信息不因容器变窄丢失。
- ExecutionRecord/Review：投入、剩余量、任务完成和目标证据分别记录；宿主关闭后重开查询 Core，不凭组件计时补写执行事实。
- ModelPrivacy/Grant：分开 Core 模型配置、宿主 agent 实际路由、人工可读范围和模型可读范围；device_only 禁止因当前模型本地而直接返回到 agent 会话。
- Receipt/Recovery：本地 committed 与外部 effects 分开展示；热重载/重连先查 receipt/cursor，历史回放纯渲染，不重新 execute。

正常、空、加载、编辑、失败、断线、无权、长内容、过期/冲突/取消的适用组合以逐屏表为准。中文字形、可见焦点、触控目标、读屏、缩放与减少动效先在 Harness 插件验证；后续 Flutter 另验，不能彼此代替。

## 旧图与文案的使用边界

七张图的审查只说明参考资产可读；Harness 插件实际布局、全 30 屏/16 弹层、异常和交互均待绘制/实现。[COPY.md](COPY.md) 仅保留业务短文案参考，若仍含独立外壳/手机表述，以本版宿主边界为准。D01 两项建议与 M01 单项是不同候选分支，局部接受需新提案与新批准，不能一起提交。

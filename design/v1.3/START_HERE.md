# 在新项目中开始Harness首期实施

本次已经决定Harness插件先行，Core保持独立Rust和全量个人事务AI能力；Flutter后续接入，仍属于完整交付目标。当前只有设计资料，下一步是在新项目实施。

1. 将本包解压到新项目或docs/design，保留相对结构；可先运行 `python tools/verify_bundle.py`。
2. 阅读 SOFTWARE_DESIGN.md、spec/06-harness-adapter-and-ui.md 和 implementation/PLAN.md，再看ui规格与七张视觉参考。不要把旧独立窗口/手机底栏直接当作宿主布局。
3. 将 IMPLEMENTATION_PROMPT.md 正文交给新项目Agent，持续推进A0–A7及完整功能账本。原型、P0或一次窄测试通过不结束完整目标。
4. A0首先以锁定版本证明树外Host+Client插件构建、安装、加载、Core连接与版本握手；不把未发布构建辅助当作已发布SDK，不要求先修改宿主主业务源码。
5. 之后完成Rust无UI纵切、Harness真实闭环、模型/MCP/恢复、P1、后续Flutter、P2及全量回归；每阶段自测通过后自动进入下一阶段。

建议目录为 crates/zodo-kernel、crates/zodo-runtime、adapters/deepseek-harness/host、adapters/deepseek-harness/client、contracts、tests与docs。后续增加zodo-ffi及apps/flutter；这里的目录是建议，不代表工程已经创建。

资料优先级：用户最新指示、1.3已确认决策和ADR、领域/运行契约、Harness桥接约束、逐屏行为与验收、视觉系统、旧参考截图。模型/宿主工具批准不能替代Core业务批准；Zodo事实不能迁入宿主Session当作唯一数据库。

# Zodo Harness首期插件设计与开发计划

版本1.3 · 2026年10月24日。用户已确认：Rust独立个人事务AI内核完整目标不变，首期UI/Agent入口改为DeepSeek Harness插件，Flutter后续提供独立跨平台客户端。清晰日程保留为视觉与交互参考。

先读 [开始入口](START_HERE.md)，再使用 [持续开发提示词](IMPLEMENTATION_PROMPT.md)。新项目无需重选Rust、首期宿主或视觉方向；按完整P0/P1/P2和后续Flutter范围持续实现、自测和修复。

## 主要文档

- [软件设计方案Word](documents/Zodo软件设计方案.docx) 与 [Markdown合集](SOFTWARE_DESIGN.md)。
- [持续开发计划Word](documents/Zodo持续开发计划.docx) 与 [A0–A7实施计划](implementation/PLAN.md)。
- [Harness适配专章](spec/06-harness-adapter-and-ui.md)、[桥接契约](contracts/HARNESS_BRIDGE.md) 与 [宿主验收矩阵](implementation/HARNESS_TEST_MATRIX.csv)。
- [30屏幕族与16弹层](ui/01-screen-and-flow-spec.md)、[AI/设置流程](ui/02-ai-and-settings-flows.md)、[UI组件与映射](ui/03-visual-spec.md)、[覆盖与缺口](ui/04-coverage-and-open-items.md)。
- [清晰日程视觉参考册](documents/清晰日程视觉参考册.docx)、[七张原图](prototype/README.md)、[tokens](prototype/design-tokens.json)、[文案](ui/COPY.md)。
- [逐屏矩阵](implementation/ACCEPTANCE_MATRIX.csv)、[关键业务验收说明](implementation/CRITICAL_SCENARIOS.json)、[演示数据](fixtures/README.md)、[协议示例](contracts/README.md)。

## 当前状态

已更新设计、计划与提示词；尚未实现Harness插件、Rust产品内核或Flutter客户端。七张图是旧独立布局的视觉参考，未生成Harness新界面。全部状态见 [BUNDLE_STATUS.json](BUNDLE_STATUS.json)，文字规格、视觉参考、可交互、真实内核、平台验证分别记录。

本包可独立解压，不依赖旧会话或旧项目。运行 `python tools/verify_bundle.py` 校验原始资料、相对链接和SHA256。1.2及更早资料是历史基线；首期入口发生冲突时以1.3和最新用户指示为准。

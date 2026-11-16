# Zodo 1.3实施约定

- Rust独立个人事务AI内核目标不变，首期UI为DeepSeek Harness的TypeScript/Cordis Host适配与React/Slots客户端；Flutter是后续独立客户端。
- 先读START_HERE.md、SOFTWARE_DESIGN.md、spec/06-harness-adapter-and-ui.md和implementation/PLAN.md，再按IMPLEMENTATION_PROMPT.md持续实施。
- Core不依赖Cordis、React、Flutter或模型/MCP SDK；适配器不复制领域校验、事务和权威状态。
- UI人工视图与Agent视图分开授权；进入宿主会话的数据先最小化；同UID高权限宿主的强隔离不能靠RPC token声称完成。
- 提案、版本、幂等、批准、回执、outbox及宿主退出/重载恢复必须验证；Session分叉/回放不自动重执行业务写入。
- 覆盖完整P0/P1/P2、30屏幕族、16弹层和后续Flutter，分阶段持续实现/自测/修复；mock、skip和静态截图不计真实产品完成。
- 普通工程选择自主推进；用户暂停/取消优先；资源或外部阻塞记录恢复点并继续独立任务，不把阻塞标完成。
- 当前包只交付设计，不要求在资料编辑任务中执行产品开发；旧项目只读，未来实施在新项目进行。

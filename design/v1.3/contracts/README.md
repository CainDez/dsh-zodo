# 运行契约与示例

[运行时与平台规范](../spec/03-runtime-contracts-and-platforms.md) 是契约基线。示例目录包含changes.commit命令、runtime生成的身份上下文、提交回执和版本冲突错误，均为说明数据，不含真实凭据。

- [提交命令](examples/01-commit-command.json)
- [运行上下文](examples/02-runtime-context.json)
- [提交回执](examples/03-commit-receipt.json)
- [版本冲突](examples/04-revision-conflict.json)

调用方不能自报runtime actor、grant或已批准状态。外部agent可提交结构化提案；application统一校验、授权、版本、幂等和事务。Query/execute/watch/getRun/cancelRun由KernelClient隔离传输差异；模型角色、MCP对象和SQLite行不是领域类型。

本包尚无可执行IDL或全量schema生成器。A0只验证最小握手和桥接请求；A1起从此规范整理版本化命令、查询、结果、事件、错误及时间类型，建立共享schema和contract tests，再按A1–A5验证daemon、嵌入及认证Web等适用部署；不能把这四个例子误当完整API或真实兼容证明。


## Harness首期适配

宿主桥接与身份/事件边界见 [HARNESS_BRIDGE.md](HARNESS_BRIDGE.md)。原生Tool和UI RPC均通过统一Zodo契约；Core批准、receipt与actor不能由模型自报。四个JSON示例保持业务schema版本，不是完整IDL或宿主已经运行的证明。

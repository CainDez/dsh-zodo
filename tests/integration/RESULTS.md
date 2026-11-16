# 跨模块 API 与安全集成回归记录

执行者：integration-tests；2026-11-07。仅写 tools/ 与 tests/integration/；既有未跟踪 asar 辅助脚本未覆盖、未运行。

## 最终结果

**12/12 集成场景组通过，0 失败；新鲜 API 50/50 checks；runner guard 8/8。**

- [脱敏完整 summary](<integration-results.json>)：可由 lead 复制至统一 artifacts 目录，无 token 或真实用户数据。
- [guard 单测记录](<runner-safety-results.json>)。
- [runner](<../../tools/run_isolated_integration.py>)、[新鲜 API suite](<../../tools/isolated_api_suite.py>)、[guard 测试](<test_runner_safety.py>)；安全设计与重跑说明见 [README](<README.md>)。
- 使用 core-tests 在清理非 DSH UI 后统一构建的 release；本任务未并发构建。二进制 15,246,848 bytes，SHA-256：`9702ccedaa0511b492bb5e69c8e380f2d12a7df3646e4043b568ab03a0e7af17`，与 Core 构建回执一致。

```text
python -B -m unittest discover -s tests/integration -p test_runner_safety.py -v
python -B tools/run_isolated_integration.py
```

两条命令均直接调用已配置的 bundled Python，退出码 0；未使用拥塞 shell 重复启动测试。guard 测试耗时 1.025s。完整集成各组耗时见 summary。原始证据仅保留在本地 gitignored 唯一树 `tests/integration/runs/isolated-6wm_92jt`，其中可能含本任务 token/合成 SQLite，不能提交或公开运行时文件。

| 已运行的原 legacy 用例（仅内存重定位副本） | 强断言覆盖 |
| --- | --- |
| [事件游标](<../../tools/test_fnd11_events.py>) | 增量/空页/future resync/limit |
| [幂等回执](<../../tools/test_h09_receipt_replay.py>) | replay 不重复任务/不涨revision，内容不符拒绝 |
| [重启游标](<../../tools/test_h10_restart_cursor.py>) | 同工作区、新实例、游标继续 |
| [日历导入](<../../tools/test_pln11_calendar.py>) | 合并/替换/坏区间原子拒绝 |
| [单包导入](<../../tools/test_dat04_import.py>) | preflight/幂等/错内容拒绝 |
| [批量导入](<../../tools/test_dat04_batch.py>) | 多包合并/replay/冲突零写入 |
| [加密同步](<../../tools/test_dat08_encrypted_sync.py>) | 无明文、正确密码导入、错密码403/零写入 |
| [双设备同步](<../../tools/test_a6_sync.py>) | roundtrip/冲突保护/显式解决/设备撤销 |
| [备份恢复](<../../tools/test_a4_restore.py>) | 仅本任务合成备份，恢复2→1事实 |
| [AI cloud-labelled stub](<../../tools/test_ai11_cloud_stub.py>) | loopback一次调用、device-only note不泄漏、3任务+2依赖、审批提交 |
| [外部日历 stub](<../../tools/test_pln16_calendar_write.py>) | 重复ID422拒绝，恰好两次外写 |

新鲜 API suite 使用另一个独立 Core，覆盖无/伪token、agent admin权限、MCP私密读取/审批/提交拒绝、跨工作区拒绝、agent撤销/恢复、伪grant拒绝零事实、合法审批提交与防重复、规划提案不提交直至批准、手动自动化预算2、调度自动化预算1。所有50个检查通过，含所有自有Core退出断言。

## 安全审查与清理

完整读取并分类26个 legacy 脚本：11个经审核可隔离重定位、15个显式跳过。原脚本普遍硬编码仓库根与证据位置，部分删除固定数据目录；8个依赖冻结归档或既存Core/bridge。原文件未修改，也没有原样直接执行。

runner 为每次执行创建唯一树，在内存重定位 ROOT/可执行文件/stub PORT；原源码SHA固定，改变后必须重审。进程只接受显式本任务data、精确serve/restore参数、随机端口；HTTP只接受自有Core或stub。环境白名单不继承真实provider secret/proxy/用户配置；只用literal dummy credentials。这是已审源码适配器，不宣称能沙箱化任意Python。

最终每组均通过自有Core handle退出后置断言；另核对19个最新fixture PID，全部不存在。本session job_list为空。初轮的18个旧fixture PID亦全部不存在。两个未实际启动的拥塞shell排队job已取消并collect killed，没有重跑活跃Core，也没有关闭其他任务或用户进程。

## 红绿回归与预期校正

初轮修复前11组：8 pass / 3 fail。两项是runner初设状态码不符正确契约：错密码实际403/PERMISSION_DENIED，重复日历ID实际422/VALIDATION_FAILED；已修正测试预期，非产品缺陷。

真实跨范围问题：AI stub给3个任务和2条依赖，成功提交但task_count=5。已报lead/core-tests，Core范围红测复现后修复为仅计TaskCreate（避免空标题被跳过时误计）；最终相同stub强断言保留3，连同两条依赖与提交结果全部通过，没有放宽断言掩盖问题。

## 仍未覆盖 / 明确跳过

- 系统通知/retry/reconcile：Core无安全disable/mock开关，不发送真实toast、不改toast行为制造通过。空Core会准备自己的toast脚本，但已运行路径无提醒入队。
- 真实本地模型、真实或付费云推理、真实日历提供商不运行。AI11和calendar write仅走本任务loopback stub。
- 冻结归档备份、live runtime/token、既存3181 bridge依赖脚本不运行；导入/同步/恢复仅合成数据。MCP、撤销、scheduler由新鲜API替代覆盖，不声称原归档依赖脚本已经运行。
- 取消竞态、崩溃恢复、clarify、完整privacy路由矩阵仍需专用deterministic stub fixture；本轮没有把真实模型测试降级后计作通过。
- 十五项逐文件跳过理由完整保留于[脱敏summary](<integration-results.json>)。

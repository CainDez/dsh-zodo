# DSH 宿主验收门槛

这不是发布说明。分支 `xiaoz/zodo-scheduler` 已实现可用路径，但没有把纯测试或桥接测试当成已登录桌面验收。

## 当前证据

- 在 `6b2d625` 实际重跑的 Node 适配器和诊断：81/81（`node --test tests/adapter/*.test.mjs tests/browser/harness-isolated-diagnostics.test.mjs`）。
- Core 最新代码对应 `078a865`：Rust workspace 159/159；该 Core 二进制在 `6b2d625` 成功通过合成数据的真实 HTTP bridge 排程冒烟，零模型调用、数据清理完成，未涉及渲染或完整 Host。
- 真实 HTTP 合成冒烟也验证了重启临时 Core 进程：重新建立适配器会话后，排程、锁定和固定记录、提案及 revision 均保持不变，零模型调用；这是 Core 重启，不是 DSH 桌面重启。
- [整周 Core 合成回归](SCHEDULER_WEEK_CORE_QA.md)验证默认追加、预约变动引发的旧批准拒绝、重新排程和两次 SQLite 关闭重开；不代表桌面重启。
- Host gateway 6/6：先用 `cargo build -p zodo-runtime --locked` 构建测试 Core，再运行 `node --test adapters/deepseek-harness/plugin/host-gateway.test.js`；自定义构建目录应先把 `ZODO_CORE_TEST_BIN` 指向该可执行文件。真实临时 Core 接入的是模拟 Host 供应商，不调用线上模型，也不是桌面集成。
- Windows x64 打包及同源 ZIP 校验在标准 Windows Server 2025 runner 通过：[Actions #4](https://github.com/CainDez/dsh-zodo/actions/runs/44359422185)，精确源码 `60f88cb6ccbbdca41909e362aece672a82dd7a67`，ZIP SHA-256 为 `e4f61d3c84a478c9fd8afd97bd50c2318c04e6e300f50b4ba0f84e7fb8880a98`。只记录校验结果和哈希，未上传 ZIP，未发布或安装。
- 同一运行以同次构建的 Core 启动隔离 Windows Chrome 夹具，**32/32** 通过。覆盖 390px 单日安排、键盘与日期切换、真实 Core 已保存而浏览器丢失回执时禁止重投，以及刷新后找回唯一一条记录。Core、夹具服务和临时数据均清理。真实插件前端和 bridge 运行在模拟 Host 中，不代表安装到 DSH Desktop。
- 真实 DSH 桌面加载、升级、卸载均未验收；不访问真实用户配置、日历或账号，不降低隔离机制来跑测试。

## 用隔离环境验收

1. 用干净检出核对精确 Git SHA，按[仓库构建流程](RELEASE.md)在隔离 Windows x64 生成分支自己的包。记录 Host 版本、源码 SHA 和 ZIP SHA-256，不拿历史 release 包替代。用包校验器比对当前源码和同次构建的 Core。分别为 Harness home、Electron user-data 和 Core 的 `ZODO_DATA_DIR` 准备新的专用目录；实际启动参数以所选 Host 版本文档为准。不对默认 profile 操作。如出现账号门槛，在这一点停止而非绕过或登录真实账号。
2. 验证从启动到「今天」/「日程」无错误；在普通宽度与 390px 切到单日、换日、回今天，检查没有横向裁切，键盘可达所有主要操作。
3. 创建任务和容量，在日期列打开「安排任务时段」。打开/取消不写；预览和批准均不写入分配；成功提交只新增一条，锁定分配与预约不变，刷新能找回。
4. 复测备选/失败：没有容量，节假限制，日期跨时区，重复本地时刻，冲突，批准过期，结果不明。核实有操作提示、输入未无故丢失、不因网络故障盲目再提交、变更提案需重新批准。
5. 用不同的小输入验证「生成计划」仅追加、「重新规划」明确允许调整未来未锁定安排。已有计划的时间或删除集合发生变化即拒绝，不能按旧批准提交。
6. 退出/重启 Host，验证当前插件版本确实装载，再验证卸载时 UI/bridge 消失且独立 Core 生命周期符合契约。清理测试 profile 和临时数据。

只有验证到的层才标通过。若无法取得合适环境，不视为产品已完成验收，也不要求开放真实账户或写入真实日历。

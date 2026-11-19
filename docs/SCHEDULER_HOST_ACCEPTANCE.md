# DSH 宿主验收门槛

这不是发布说明。分支 `xiaoz/zodo-scheduler` 已实现可用路径，但没有把纯测试或桥接测试当成已登录桌面验收。

## 当前证据

- 在 `6b2d625` 实际重跑的 Node 适配器和诊断：81/81（`node --test tests/adapter/*.test.mjs tests/browser/harness-isolated-diagnostics.test.mjs`）。
- 历史 Core 检查点 `078a865`：Rust workspace 159/159；该 Core 二进制在 `6b2d625` 成功通过合成数据的真实 HTTP bridge 排程冒烟，零模型调用、数据清理完成，未涉及渲染或完整 Host。
- 真实 HTTP 合成冒烟也验证了重启临时 Core 进程：重新建立适配器会话后，排程、锁定和固定记录、提案及 revision 均保持不变，零模型调用；这是 Core 重启，不是 DSH 桌面重启。
- [整周 Core 合成回归](SCHEDULER_WEEK_CORE_QA.md)验证默认追加、预约变动引发的旧批准拒绝、重新排程和两次 SQLite 关闭重开；不代表桌面重启。
- Host gateway 6/6：先用 `cargo build -p zodo-runtime --locked` 构建测试 Core，再运行 `node --test adapters/deepseek-harness/plugin/host-gateway.test.js`；自定义构建目录应先把 `ZODO_CORE_TEST_BIN` 指向该可执行文件。真实临时 Core 接入的是模拟 Host 供应商，不调用线上模型，也不是桌面集成。
- Windows x64 打包及同源 ZIP 校验在标准 Windows Server 2025 runner 通过：[Actions #4](https://github.com/CainDez/dsh-zodo/actions/runs/44359422185)，精确源码 `60f88cb6ccbbdca41909e362aece672a82dd7a67`，ZIP SHA-256 为 `e4f61d3c84a478c9fd8afd97bd50c2318c04e6e300f50b4ba0f84e7fb8880a98`。只记录校验结果和哈希，未上传 ZIP，未发布或安装。
- 同一运行以同次构建的 Core 启动隔离 Windows Chrome 夹具，**32/32** 通过。覆盖 390px 单日安排、键盘与日期切换、真实 Core 已保存而浏览器丢失回执时禁止重投，以及刷新后找回唯一一条记录。Core、夹具服务和临时数据均清理。真实插件前端和 bridge 运行在模拟 Host 中，不代表安装到 DSH Desktop。
- 版本升级的旧源码固定为 `c3f3d0d63c7467dee14c241c2c05405dee7c2130`（插件 `0.2.0-dev.1`），新包为本分支 `0.3.0-dev.1`。Linux 隔离 Core 用旧版官方 CLI 建立合成记录，复现新版无法读取旧隐私配置；修复后同一 SQLite 中的固定预约、锁定与可移动安排、已确认提案及业务 revision 均保留。新版只新增一条经批准的安排，再重启仍一致。旧模型路由和诊断设置不等于新版授权；读取不改写，正常事务写入才保存新格式，之后不保证可用旧版打开。此 Linux 结果不等于 Desktop 插件升级。
- [Windows Actions #5](https://github.com/CainDez/dsh-zodo/actions/runs/44439847145) 在 `a95c4e9d3abb3c5dbabddc569bf5b45fd8206b2b` 实际构建、校验两版 x64 包，旧版 ZIP SHA-256 `c11b6e8852232506e964e8a65b7494d4a8f64c85e001128157113e9069617d74`，新版 `b99f43b0b9fec0a1920b5a7e809535bcaadd3c04b0fd24fbd235177349549139`。包内 Core 与测试所用同次构建 Core 一致。Windows 同一 SQLite 合成工作区升级保留旧记录、只新增一次，重启快照一致、清理通过；同次 Windows Chrome 夹具 32/32 通过。CI 没有上传发布包，也没有启动真实 DSH Desktop。
- 真实 DSH 桌面加载、升级、卸载均未验收；不访问真实用户配置、日历或账号，不降低隔离机制来跑测试。

## 用隔离环境验收

1. 用干净检出核对精确 Git SHA，按[仓库构建流程](RELEASE.md)在隔离 Windows x64 生成分支自己的包。记录 Host 版本、源码 SHA 和 ZIP SHA-256，不拿历史 release 包替代。用包校验器比对当前源码和同次构建的 Core。分别为 Harness home、Electron user-data 和 Core 的 `ZODO_DATA_DIR` 准备新的专用目录；实际启动参数以所选 Host 版本文档为准。不对默认 profile 操作。如出现账号门槛，在这一点停止而非绕过或登录真实账号。
2. 验证从启动到「今天」/「日程」无错误；在普通宽度与 390px 切到单日、换日、回今天，检查没有横向裁切，键盘可达所有主要操作。
3. 创建任务和容量，在日期列打开「安排任务时段」。打开/取消不写；预览和批准均不写入分配；成功提交只新增一条，锁定分配与预约不变，刷新能找回。
4. 复测备选/失败：没有容量，节假限制，日期跨时区，重复本地时刻，冲突，批准过期，结果不明。核实有操作提示、输入未无故丢失、不因网络故障盲目再提交、变更提案需重新批准。
5. 用不同的小输入验证「生成计划」仅追加、「重新规划」明确允许调整未来未锁定安排。已有计划的时间或删除集合发生变化即拒绝，不能按旧批准提交。
6. 先在 Windows x64 以第 1 步的隔离方式从上述固定旧源码构建并核对包版本与哈希，再用该旧版 Zodo 包在同一隔离 profile 中建立合成基线：固定预约、锁定安排、可移动任务和一次已确认的计划。保存这几类记录的 ID 和日期时间，并记录 Core 是否由 Host 启动。按官方支持的流程升级 **Zodo 插件**到当前分支包；不要清空或重置数据来让升级通过。
7. 完全退出并重启 DSH Desktop，核对插件和 Core 实际版本，复查基线记录与约束未改变，并新建一次安排，确认只产生一条记录。再通过官方流程卸载 **Zodo 插件**，确认入口和 bridge 消失，独立 Core 未被误终止；清理合成 profile 和临时数据。没有可复现旧包时，标记「升级未验证」，不以重启或重装替代。

只有验证到的层才标通过。若无法取得合适环境，不视为产品已完成验收，也不要求开放真实账户或写入真实日历。

### 真实桌面启动的前置检查（尚未执行）

- 只在一次性 Windows 测试机或专用隔离账号内运行官方 Desktop，核对实际安装包的版本、签名与哈希。除 Core 数据目录外，先确认打包版实际采用独立的 Harness home 和 Electron user-data；不要用开发启动变量代替现场验证。启动可能注册 `dsh://` 协议，不能在日常环境试跑。
- 已从 [DSH 0.2.2-rc.5 固定源码](https://github.com/deepseek-ai/deepseek-harness/blob/9c2670d3bce5275e0d1610272958121af87787bb/apps/desktop/src/client/WelcomePage.tsx#L202-L208) 确认：欢迎页进入 API Key 子页后可选择「稍后」，冷启动后再次走同一入口。不填真实或伪造凭据。如果实际安装包没有显示该入口，停止本轮。
- 按[官方 Desktop 安装插件说明](https://github.com/deepseek-ai/deepseek-harness/blob/9c2670d3bce5275e0d1610272958121af87787bb/apps/desktop/README.md)先启动初始化、退出，再用随 Desktop 提供的 CLI 操作 `desktop` profile。Zodo 的 ZIP 先验哈希再解包到测试目录；使用安装器实际支持的本地包格式，不假定 ZIP 可直接安装。真实启动前仍需明确遥测和更新的处理；若出现未安排的权限、更新或服务访问，停止并记录。
- [官方固定版本的开发指南](https://github.com/deepseek-ai/deepseek-harness/blob/9c2670d3bce5275e0d1610272958121af87787bb/apps/desktop/README.md#L331-L345)也支持解包版做中间验证，可检查真实 Electron Host、随包运行时和插件加载；单独记录结果，不把它当作官方安装版验收。官方安装版会在启动时检查更新；[产品遥测](https://github.com/deepseek-ai/deepseek-harness/blob/9c2670d3bce5275e0d1610272958121af87787bb/packages/client/product-analytics/README.md#L175-L195)是另一条路径。只用合成数据，先确认隔离环境中的两者行为，不因为跳过登录就假定不联网。

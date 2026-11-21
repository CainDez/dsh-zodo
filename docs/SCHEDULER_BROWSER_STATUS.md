# 日程浏览器验收状态

已添加隔离浏览器用例：在 Asia/Shanghai 视图中检查前后翻日程、回今天、容量和任务分配详情使用键盘/鼠标打开，导航不发送写入命令，Core 快照和 revision 均不变。

2026-11-18 Linux：锁定依赖安装、临时 Core 构建、浏览器套件语法检查通过；Chromium 启动时遇到 `process_singleton_posix.cc: socket() failed: Operation not permitted`，未进入任何 UI 用例。该结果不是浏览器通过也不是产品断言失败。下一次选择可运行 Chromium 的隔离测试环境，重跑整套。真实 DSH 桌面加载、登录和生命周期仍待单独验收。


2026-11-18 Windows：在 [Actions #4](https://github.com/CainDez/dsh-zodo/actions/runs/44359422185) 的精确提交 `60f88cb`，使用同次 Windows 构建的 Core 和预装 Chrome 跑完隔离套件，32 项全通过。此前 Linux 启动阻塞只是环境记录，不否定这次浏览器通过。真实 DSH Desktop 仍未验收。

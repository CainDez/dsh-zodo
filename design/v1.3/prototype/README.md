# 清晰日程旧视觉参考与 Harness 适配索引

版本 1.3 · 2026-10-24。用户保留第一套清晰日程；首期改为 Harness UI 插件，独立 Rust Core 和全功能目标不变，Flutter 后续作为独立客户端。以下七张原尺寸 PNG 未改动，仅参考信息层级、差异表达和局部视觉，不是 Harness 插件新截图，不计插件已出图、已装载或已实现交互。

Harness 内使用 React + 宿主 Slots/详情/批准视图，经 Host RPC bridge 访问 Core；五业务入口在插件自有内容区组织，不要求更换宿主全局导航。原图的整套左栏、浏览器尺寸和手机五底栏不直接搬入宿主。M01 仅是后续窄屏/手机确认参考，不能证明 Harness 拥有原生移动后台或通知能力。

| 旧参考编号 | 信息层级参考 | 原图（非 Harness 截图） |
| --- | --- | --- |
| D01 | 今日工作台 | [原尺寸PNG](screens/d01-today.png) |
| D02 | 收集箱与任务详情 | [原尺寸PNG](screens/d02-inbox.png) |
| D03 | 目标与任务分解 | [原尺寸PNG](screens/d03-goal.png) |
| D04 | 日程与计划预览 | [原尺寸PNG](screens/d04-plan.png) |
| D05 | 回顾与执行反馈 | [原尺寸PNG](screens/d05-review.png) |
| D06 | 模型隐私与代理权限 | [原尺寸PNG](screens/d06-privacy.png) |
| M01 | 后续窄屏/Flutter 手机计划确认 | [原尺寸PNG](screens/m01-mobile-plan.png) |

详见 [Harness 页面流程](SCREEN_FLOW.md)、[局部样式 tokens](design-tokens.json)、[Harness 视觉规范](../ui/03-visual-spec.md) 和 [覆盖与未完成项](../ui/04-coverage-and-open-items.md)。实际首期截图、React 控件、RPC 和真实 Core 证据均待补；原图通过审查不升级为插件验收。下列图片中的模型就绪、授权、执行记录均为演示状态。

## D01 今日工作台

![今日工作台](screens/d01-today.png)

## D02 收集箱与任务详情

![收集箱与任务详情](screens/d02-inbox.png)

## D03 目标与任务分解

![目标与任务分解](screens/d03-goal.png)

## D04 日程与计划预览

![日程与计划预览](screens/d04-plan.png)

## D05 回顾与执行反馈

![回顾与执行反馈](screens/d05-review.png)

## D06 模型隐私与代理权限

![模型隐私与代理权限](screens/d06-privacy.png)

## M01 后续窄屏/手机计划确认参考

![手机计划确认](screens/m01-mobile-plan.png)

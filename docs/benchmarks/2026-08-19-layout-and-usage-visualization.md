# 排版与 Token 可视化回归

日期：2026-08-19

## 目标

- 减少推理后端页面的等宽、纵向卡片堆叠感。
- 让用户无需阅读请求表即可理解 Token 趋势、构成和主要来源。
- 保持后台常驻预算，不为图表引入持续计算或大型运行时依赖。

## 开源方案调研

| 方案 | 官方定位 | 本轮结论 |
| --- | --- | --- |
| [uPlot](https://github.com/leeoniya/uPlot) | MIT，约50 KB，面向高性能时序图，无内置动画 | 采用其“预聚合、少绘制对象、无动画”原则 |
| [Recharts](https://github.com/recharts/recharts) | MIT，React + D3的声明式SVG组件 | 组合能力良好，但当前两张小图无需增加依赖 |
| [Chart.js](https://github.com/chartjs/Chart.js) | MIT，Canvas图表 | 功能超过首版需求，不引入 |
| [Apache ECharts](https://github.com/apache/echarts) | Apache-2.0，完整交互式可视化平台 | 功能和体积均超出当前需求，不引入 |

最终使用HAL100项目内静态SVG组件，没有复制第三方源码，也没有修改依赖清单。

## 实现约束

- 趋势图最多读取当前查询中的最近30条请求。
- 只绘制一条面积路径和两条折线，不创建逐点交互节点。
- 环图固定三个分段：非缓存输入、缓存输入、输出。
- 不使用动画、`requestAnimationFrame`、`ResizeObserver`或图表定时器。
- React Query保持无限`staleTime`并关闭窗口聚焦刷新；只在进入页面或点击刷新时查询。
- 有数据时请求明细默认折叠，展开后的表格使用420 px内部滚动区。

## 验证结果

- `pnpm check`：通过。
- 桌面React测试：15/15通过。
- Agent Kernel测试：22/22通过。
- 生产构建：JS 409.00 kB，gzip 119.60 kB；未增加图表依赖或独立图表Chunk。
- Playwright：1280×800亮色、1280×800深色和880×620最小窗口均无横向溢出，控制台无错误或警告。
- 干净重启后Gateway健康检查：HTTP 200。
- HAL100桌面进程连续5次一秒采样：CPU均为0.0%，RSS为133.3–133.5 MiB，满足UI打开且静止时低于180 MiB的预算。

视觉产物位于`output/playwright/layout-visualization/`。

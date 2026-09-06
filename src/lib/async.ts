// 异步加载辅助：sleep 用于「刷新按钮最小可见时长」——本地/mock 数据常瞬间返回，
// 旋转动画一闪而过；各列表页在 finally 里用 sleep(400 - elapsed) 补足到 400ms，
// 保证 spinner 稳定可见又不拖慢内容渲染（内容已先于 sleep 落库展示）。

export const sleep = (ms: number): Promise<void> =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

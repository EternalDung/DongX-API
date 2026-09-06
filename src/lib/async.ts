// 异步加载辅助：保证加载态至少可见 minMs 毫秒。
// 本地/mock 数据常瞬间返回，导致 loading 真值态不足一帧就被清空，
// 刷新/骨架屏的旋转动画一闪而过（请求日志因数据量大耗时更长所以看得见）。
// 用 withMinDuration 包裹后，spinner 至少持续 minMs，动画稳定可见。

export const sleep = (ms: number): Promise<void> =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

export async function withMinDuration<T>(
  p: Promise<T>,
  minMs: number,
): Promise<T> {
  const result = await Promise.all([p, sleep(minMs)]);
  return result[0];
}

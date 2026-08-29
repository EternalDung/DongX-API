import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * 把监听地址格式化成可展示/可点击的 base URL。
 *
 * - `0.0.0.0`（监听所有网卡）对外展示成 localhost，否则用户点不开链接
 * - `::` / `::0` 这类 IPv6 通配同理
 * - host 为空时回退到 127.0.0.1，避免出现 `http://:9842` 这种残缺串
 */
export function formatListenUrl(host: string | null | undefined, port: number | null | undefined) {
  const safeHost = !host || host.trim() === "" ? "127.0.0.1" : host.trim();
  const displayHost =
    safeHost === "0.0.0.0" || safeHost === "::" || safeHost === "::0"
      ? "localhost"
      : safeHost;
  // IPv6 地址在 URL 里必须加方括号，否则冒号会跟端口分隔符冲突
  const urlHost = displayHost.includes(":") ? `[${displayHost}]` : displayHost;
  return `http://${urlHost}:${port ?? ""}/v1`;
}

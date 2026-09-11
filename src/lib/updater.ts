// 应用自动更新封装
//
// 链路：check() 拉 GitHub Releases 的 latest.json（端点 + pubkey 由 tauri.conf.json 提供）→
// downloadAndInstall() 流式下载并触发安装 → relaunch() 重启应用（mac/linux 必需；
// windows 的 NSIS 安装包默认 restartAfterInstall=true，会自启，所以 relaunch 是兜底）。
//
// 全部失败静默吞掉：自动更新是体验加分项，不应阻塞或打扰用户。
import { check } from "@tauri-apps/plugin-updater";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

/** 给 UI 层使用的精简更新信息 */
export interface UpdateInfo {
  currentVersion: string;
  version: string;
  date?: string;
  body?: string;
}

/** 下载进度事件（已把 Started 的 contentLength 与 Progress 的 chunkLength 累计成百分比友好形式） */
export interface DownloadProgress {
  event: DownloadEvent["event"];
  /** 累计已下载字节（仅 Progress 阶段累加，Started 时归零） */
  downloaded: number;
  /** 总字节（来自 Started.contentLength，可选——某些服务器不发 Content-Length） */
  total: number | undefined;
}

/** 取当前版本号（来自 tauri.conf.json 的 version 字段，由后端打包时注入） */
export async function getCurrentVersion(): Promise<string> {
  try {
    return await getVersion();
  } catch {
    return "0.0.0";
  }
}

/**
 * 检查更新。
 * - 返回 null 表示已是最新（或拉取失败），UI 显示「已是最新」
 * - 返回 UpdateInfo 表示有可用更新，UI 渲染版本号 + 更新说明
 */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  const update = await check();
  if (!update) return null;
  return {
    currentVersion: update.currentVersion,
    version: update.version,
    date: update.date,
    body: update.body,
  };
}

/**
 * 下载并安装更新。下载阶段持续回调 onProgress。
 *
 * 平台差异：
 * - windows：install 会在安装器拉起后退出当前进程，downloadAndInstall 可能不会正常 resolve；
 *   NSIS 默认 restartAfterInstall=true，安装完自动重启，所以这里 relaunch 仅作为兜底。
 * - macOS / linux：需要显式 relaunch() 才会在新版本上启动。
 */
export async function downloadAndInstall(
  onProgress?: (p: DownloadProgress) => void,
): Promise<void> {
  const update = await check();
  if (!update) {
    throw new Error("当前已是最新版本，无需更新");
  }

  let downloaded = 0;
  let total: number | undefined;

  await update.downloadAndInstall((event: DownloadEvent) => {
    if (event.event === "Started") {
      total = event.data.contentLength;
      downloaded = 0;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
    }
    onProgress?.({
      event: event.event,
      downloaded,
      total,
    });
  });

  // macOS / linux 需要重启；windows 上若进程已被安装器结束，此处不会执行。
  try {
    await relaunch();
  } catch {
    // ignore: windows 上常因进程已退出而抛错
  }
}

/** 手动重启应用：安装完成后进程若未自行退出，由弹窗的「重启应用」按钮调用 */
export async function relaunchApp(): Promise<void> {
  try {
    await relaunch();
  } catch {
    // 非 Tauri 环境或系统拒绝时静默
  }
}

/** 字节数格式化（更新弹窗与侧边栏进度共用） */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** 下载百分比；服务器未给 Content-Length 时返回 0，UI 退化为不确定态 */
export function downloadPercent(
  downloaded: number,
  total: number | undefined,
): number {
  if (!total || total <= 0) return 0;
  return Math.min(100, Math.round((downloaded / total) * 100));
}

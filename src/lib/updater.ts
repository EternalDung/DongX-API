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
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

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

/**
 * 启动后静默检查：发现更新则发系统通知，提示用户前往「设置 → 更新」手动升级。
 *
 * 设计取舍：
 * - 不弹窗、不抢焦点：每次启动都打扰会让用户把通知关掉
 * - 失败一律静默：网络/插件未注册等情况都不影响正常使用
 * - 通知权限按需请求：只有真的有更新才申请，避免一开始就弹权限框
 */
export async function startupAutoCheck(): Promise<void> {
  try {
    const info = await checkForUpdate();
    if (!info) return;

    let granted = await isPermissionGranted();
    if (!granted) {
      const perm = await requestPermission();
      granted = perm === "granted";
    }
    if (!granted) return;

    await sendNotification({
      title: `DongX v${info.version} 已发布`,
      body: `当前 v${info.currentVersion}。前往「设置 → 更新」一键升级。`,
    });
  } catch {
    // 静默吞掉
  }
}
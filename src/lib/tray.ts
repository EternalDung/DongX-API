import { getCurrentWindow } from "@tauri-apps/api/window";
import { settingsApi } from "./api";

let initialized = false;

/**
 * 设置窗口的「最小化到托盘」行为。
 *
 * Tauri v2 没有 `onMinimize` 事件，也无法拦截原生最小化按钮，因此用
 * 「窗口失焦 + 判断是否已最小化」间接实现：仅当窗口确实被最小化
 * （点击了最小化按钮）时才隐藏到托盘；普通切到别的应用（失焦但未最小化）
 * 不会触发，避免变成「点开别的窗口就消失」。
 *
 * 关闭到托盘在 Rust 侧拦截 `CloseRequested`，见 `src-tauri/src/tray.rs`。
 */
export function setupTrayBehaviors() {
  if (initialized) return;
  initialized = true;

  const win = getCurrentWindow();

  win
    .onFocusChanged(async ({ payload: focused }) => {
      // 仅关心「失去焦点」；重新获得焦点无需处理
      if (focused) return;
      try {
        const settings = await settingsApi.get();
        if (!settings.minimize_to_tray) return;
        // 用 isMinimized 区分「最小化」与「切到别的窗口」
        if (await win.isMinimized()) {
          await win.hide();
        }
      } catch {
        // 读取设置或隐藏失败时忽略，托盘菜单仍可手动找回窗口
      }
    })
    .catch(() => {
      // 监听器注册失败（极少数环境）时忽略，不影响主流程
    });
}

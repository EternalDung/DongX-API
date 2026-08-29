//! 系统托盘图标与菜单。
//!
//! 托盘是「最小化/关闭到托盘」的前置条件：窗口隐藏后必须有一个入口把它
//! 找回，否则应用会「消失」。因此托盘图标在 setup 阶段无条件创建。
//!
//! - 关闭到托盘的 `CloseRequested` 拦截放在 `lib.rs` 的 Builder 上（见
//!   `on_window_event`），因为它需要 `&Window` 且闭包需满足 `'static`。
//! - 最小化到托盘因 Tauri v2 没有 `Minimize` 事件，放到前端用「失焦 +
//!   判断是否已最小化」间接实现（见前端 `src/lib/tray.ts`）。

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

/// 创建系统托盘图标与菜单。必须在 `setup` 内调用（`app` 为 `&App`）。
pub fn create(app: &tauri::App) -> tauri::Result<()> {
    // 没有窗口图标时不创建托盘（理论上不会走到，默认图标一定存在）
    let Some(icon) = app.default_window_icon().cloned() else {
        return Ok(());
    };

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "隐藏到托盘", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 DongX", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quit" => app.exit(0),
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

use std::time::Duration;

use openusage_lib::{accounts, claude, series};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

#[tauri::command]
fn get_usage() -> claude::UsageReport {
    claude::scan()
}

#[tauri::command]
fn get_accounts() -> Vec<accounts::AccountInfo> {
    accounts::all_accounts()
}

#[tauri::command]
fn get_claude_accounts() -> Vec<accounts::AccountInfo> {
    accounts::claude_accounts()
}

#[tauri::command]
fn get_series() -> series::Series {
    series::build(&claude::all_usage_files())
}

#[tauri::command]
fn get_roots() -> Vec<String> {
    claude::config_roots()
        .into_iter()
        .map(|(label, path)| format!("{label} — {}", path.display()))
        .collect()
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn tooltip(report: &claude::UsageReport) -> String {
    if report.models.is_empty() {
        return "OpenUsage — no Claude Code sessions found".into();
    }
    format!(
        "OpenUsage — {} req · ${:.2} · {:.1}M tokens",
        report.total_requests,
        report.total_cost_usd,
        report.total_tokens as f64 / 1_000_000.0
    )
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_usage, get_roots, get_accounts, get_claude_accounts, get_series])
        .setup(|app| {
            let handle = app.handle().clone();

            // Scan now, then poll: the tray tooltip must not wait on a UI render.
            let refresh_handle = handle.clone();
            let refresh = move || {
                let report = claude::scan();
                if let Some(tray) = refresh_handle.tray_by_id("main") {
                    let _ = tray.set_tooltip(Some(tooltip(&report)));
                }
            };
            refresh();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                    refresh();
                }
            });

            let menu = Menu::with_items(
                &handle,
                &[
                    &MenuItem::with_id(&handle, "show", "Open OpenUsage", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(&handle)?,
                    &MenuItem::with_id(&handle, "quit", "Quit", true, None::<&str>)?,
                ],
            )?;

            TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    _ => show_window(app),
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_window(tray.app_handle());
                    }
                })
                .build(&handle)?;

            // Start hidden: the popover only appears when the tray icon is clicked.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the popover hides it; the tray keeps running.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running OpenUsage");
}
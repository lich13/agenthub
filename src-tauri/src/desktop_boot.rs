use std::{
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{PhysicalPosition, PhysicalSize, Runtime, Size, WebviewWindow};

use agenthub_core::platform::PlatformPaths;

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const DESKTOP_RUNTIME_MARKER_SCRIPT: &str = r#"
window.__AGENTHUB_DESKTOP_RUNTIME__ = true;
"#;

const DESKTOP_BOOT_PROBE_SCRIPT: &str = r#"
(function () {
  var root = document.getElementById("root");
  var bodyText = (document.body && document.body.innerText ? document.body.innerText : "").replace(/\s+/g, " ").trim();
  return {
    readyState: document.readyState,
    title: document.title,
    desktopRuntime: window.__AGENTHUB_DESKTOP_RUNTIME__ === true,
    bootMounted: Boolean(window.__AGENTHUB_BOOT__ && window.__AGENTHUB_BOOT__.mounted),
    rootChildren: root ? root.children.length : -1,
    rootClass: root && root.firstElementChild ? root.firstElementChild.className : "",
    bodyTextLength: bodyText.length,
    hasMainShell: Boolean(document.querySelector(".app-shell")),
    hasDesktopNav: ["Codex", "Grok Build", "Probe", "设置"].every(function (label) {
      return Boolean(document.querySelector('.side-nav button[aria-label="' + label + '"]'));
    }),
    hasMachineSelector: Boolean(document.querySelector('[aria-label="当前机器"]'))
  };
})()
"#;

/// Reveal an already initialized window without touching its geometry.
///
/// Reapplying maximize/position/size during Ready, reopen, or a machine switch
/// makes macOS animate the window repeatedly and can block the WebView for
/// several seconds. Geometry belongs to the one-time startup path below.
pub(crate) fn reveal_main_window<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// Initialize the main window once after the explicit WebView has been built.
/// Native maximize is preferred; the work-area size is a single bounded
/// fallback for platforms where maximize does not resize a newly-created
/// hidden window.
pub(crate) fn initialize_main_window<R: Runtime>(window: &WebviewWindow<R>) {
    let _ = window.maximize();
    if !window.is_maximized().unwrap_or(false) {
        fit_main_window_to_work_area(window);
    }
}

fn fit_main_window_to_work_area<R: Runtime>(window: &WebviewWindow<R>) {
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => Some(monitor),
        _ => window.primary_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor else {
        return;
    };
    let work_area = monitor.work_area();
    if work_area.size.width == 0 || work_area.size.height == 0 {
        return;
    }
    let _ = window.set_position(PhysicalPosition::new(
        work_area.position.x,
        work_area.position.y,
    ));
    let _ = window.set_size(Size::Physical(PhysicalSize::new(
        work_area.size.width,
        work_area.size.height,
    )));
}

fn append_desktop_app_log(message: &str) {
    let paths = PlatformPaths::desktop_current();
    if std::fs::create_dir_all(&paths.log_dir).is_err() {
        return;
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_dir.join("agenthub.log"))
    else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let _ = writeln!(file, "{timestamp} {message}");
}

pub(crate) fn schedule_desktop_boot_probe<R: tauri::Runtime>(window: &WebviewWindow<R>) {
    let probe_window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if let Err(err) = probe_window.eval_with_callback(DESKTOP_BOOT_PROBE_SCRIPT, |payload| {
            append_desktop_app_log(&format!("desktop_boot_probe {payload}"));
        }) {
            append_desktop_app_log(&format!("desktop_boot_probe_error {err}"));
        }
    });
}

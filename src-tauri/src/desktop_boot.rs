use tauri::{PhysicalPosition, PhysicalSize, Runtime, Size, WebviewWindow};

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const DESKTOP_RUNTIME_MARKER_SCRIPT: &str = r#"
window.__AGENTHUB_DESKTOP_RUNTIME__ = true;
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

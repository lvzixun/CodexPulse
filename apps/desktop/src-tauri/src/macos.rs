//! macOS menu-bar host. Geometry comes from the native status item, never from
//! the floating Windows anchor. The shared WebView is created only on demand.
use crate::{geometry::Area, platform};
use tauri::Manager;

#[derive(Default)]
pub struct StatusState {
    desired: Option<crate::menubar_status::Status>,
    applied: Option<crate::menubar_status::Status>,
    queued: bool,
}

pub fn icon() -> tauri::image::Image<'static> {
    // A 22-point template pulse at Retina resolution. Only alpha matters to
    // AppKit; it supplies the correct foreground for the current menu bar.
    let mut rgba = vec![0u8; 44 * 44 * 4];
    let points = [
        (5., 23.),
        (15., 23.),
        (20., 10.),
        (27., 34.),
        (32., 23.),
        (39., 23.),
    ];
    for y in 0..44 {
        for x in 0..44 {
            let on_line = points.windows(2).any(|segment| {
                let (ax, ay) = segment[0];
                let (bx, by) = segment[1];
                let dx = bx - ax;
                let dy = by - ay;
                let t = (((x as f64 - ax) * dx + (y as f64 - ay) * dy) / (dx * dx + dy * dy))
                    .clamp(0., 1.);
                (x as f64 - ax - t * dx).hypot(y as f64 - ay - t * dy) <= 1.6
            });
            if on_line {
                rgba[(y * 44 + x) * 4 + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 44, 44)
}

pub fn set_status_symbol(tray: &tauri::tray::TrayIcon) {
    let configured = tray.with_inner_tray_icon(|inner| {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return false;
        };
        let Some(item) = inner.ns_status_item() else {
            return false;
        };
        let Some(button) = item.button(mtm) else {
            return false;
        };
        let Some(symbol) =
            objc2_app_kit::NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &objc2_foundation::NSString::from_str("waveform.path.ecg"),
                Some(&objc2_foundation::NSString::from_str("CodexPulse 状态")),
            )
        else {
            return false;
        };
        let configuration =
            objc2_app_kit::NSImageSymbolConfiguration::configurationWithPointSize_weight(18.0, 0.0);
        let image = symbol
            .imageWithSymbolConfiguration(&configuration)
            .unwrap_or(symbol);
        image.setTemplate(true);
        button.setImage(Some(&image));
        button.setFont(Some(
            &objc2_app_kit::NSFont::monospacedDigitSystemFontOfSize_weight(13.0, 0.0),
        ));
        button.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeft);
        true
    });
    if matches!(configured, Ok(true)) {
        // Recompute the native hit target after SF Symbols sets its optical size.
        let _ = tray.set_title(None::<&str>);
    }
}

pub fn observe_deactivation(app: &tauri::AppHandle) {
    // App deactivation also covers clicks on the desktop, Command-Tab and
    // system menus, for which a window-focus event alone is insufficient.
    let handle = app.clone();
    let block = block2::RcBlock::new(
        move |_: std::ptr::NonNull<objc2_foundation::NSNotification>| {
            platform::hide(&handle);
        },
    );
    // AppKit posts this notification on the main thread; the captured handle
    // is Send. The observer is retained until explicit main-thread shutdown.
    let observer = unsafe {
        objc2_foundation::NSNotificationCenter::defaultCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(objc2_app_kit::NSApplicationDidResignActiveNotification),
                None,
                None,
                &block,
            )
    };
    app.state::<platform::WindowHost>()
        .deactivate_observer
        .store(
            objc2::rc::Retained::into_raw(observer).cast::<objc2::runtime::AnyObject>() as usize,
            std::sync::atomic::Ordering::Relaxed,
        );
}

pub fn shutdown(app: &tauri::AppHandle) {
    let pointer = app
        .state::<platform::WindowHost>()
        .deactivate_observer
        .swap(0, std::sync::atomic::Ordering::Relaxed);
    if pointer != 0 {
        // This is the retained observer installed above, taken exactly once;
        // RunEvent shutdown callbacks run on the same AppKit main thread.
        unsafe {
            let observer =
                objc2::rc::Retained::<objc2::runtime::AnyObject>::from_raw(pointer as *mut _)
                    .expect("deactivation observer");
            objc2_foundation::NSNotificationCenter::defaultCenter().removeObserver(&observer);
        }
    }
}

pub fn apply_appearance(window: &tauri::WebviewWindow, theme: &str) {
    let theme = theme.to_owned();
    let native_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        use objc2_app_kit::NSAppearanceCustomization;
        if let Ok(pointer) = native_window.ns_window() {
            // AppKit owns this window; restrict access to its main thread.
            let window = unsafe { &*pointer.cast::<objc2_app_kit::NSWindow>() };
            let name = match theme.as_str() {
                "light" => Some(unsafe { objc2_app_kit::NSAppearanceNameAqua }),
                "dark" => Some(unsafe { objc2_app_kit::NSAppearanceNameDarkAqua }),
                _ => None,
            };
            let appearance = name.and_then(objc2_app_kit::NSAppearance::appearanceNamed);
            window.setAppearance(appearance.as_deref());
        }
    });
}

pub fn prepare_window(window: &tauri::WebviewWindow) {
    let native_window = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(pointer) = native_window.ns_window() {
            // Tauri owns this NSWindow for the captured WebviewWindow lifetime;
            // AppKit access stays on the main thread and does not retain it.
            let window = unsafe { &*pointer.cast::<objc2_app_kit::NSWindow>() };
            if let Some(content) = window.contentView() {
                content.setWantsLayer(true);
                if let Some(layer) = content.layer() {
                    layer.setCornerRadius(20.0);
                    layer.setMasksToBounds(true);
                }
            }
            window.setCollectionBehavior(
                objc2_app_kit::NSWindowCollectionBehavior::MoveToActiveSpace
                    | objc2_app_kit::NSWindowCollectionBehavior::FullScreenAuxiliary
                    | objc2_app_kit::NSWindowCollectionBehavior::Transient,
            );
        }
    });
}

pub fn update_status(app: &tauri::AppHandle) {
    let Some(backend) = app.try_state::<crate::backend::Backend>() else {
        return;
    };
    let Ok(snapshot) = backend.snapshot.read() else {
        return;
    };
    let status = crate::menubar_status::render(
        &snapshot,
        chrono::Utc::now(),
        crate::language::effective(&snapshot.settings.language),
    );
    drop(snapshot);
    let host = app.state::<platform::WindowHost>();
    let Ok(mut state) = host.status.lock() else {
        return;
    };
    state.desired = Some(status);
    if state.queued || state.applied == state.desired {
        return;
    }
    state.queued = true;
    drop(state);
    let handle = app.clone();
    if app
        .run_on_main_thread(move || {
            let host = handle.state::<platform::WindowHost>();
            // Read the latest desired value, coalescing updates while AppKit is busy.
            let desired = host
                .status
                .lock()
                .ok()
                .and_then(|state| state.desired.clone());
            let applied = desired.filter(|status| {
                handle.tray_by_id("pulse-tray").is_some_and(|tray| {
                    // Retain the native template SF Symbol. A new raster icon would
                    // lose AppKit's automatic light/dark menu-bar foreground.
                    let applied = tray.set_title(Some(&status.title)).is_ok()
                        && tray.set_tooltip(Some(&status.tooltip)).is_ok();
                    if applied {
                        let label = status.tooltip.clone();
                        let _ = tray.with_inner_tray_icon(move |inner| {
                            use objc2_app_kit::NSAccessibility;
                            if let Some(mtm) = objc2::MainThreadMarker::new()
                                && let Some(button) =
                                    inner.ns_status_item().and_then(|item| item.button(mtm))
                            {
                                button.setAccessibilityLabel(Some(
                                    &objc2_foundation::NSString::from_str(&label),
                                ));
                            }
                        });
                    }
                    applied
                })
            });
            if let Ok(mut state) = host.status.lock() {
                if applied.is_some() {
                    state.applied = applied;
                }
                state.queued = false;
            }
        })
        .is_err()
        && let Ok(mut state) = host.status.lock()
    {
        state.queued = false;
    }
}

pub fn toggle(app: &tauri::AppHandle) {
    if app
        .get_webview_window("pulse")
        .is_some_and(|w| w.is_visible().unwrap_or(false))
    {
        platform::hide(app);
        return;
    }
    // The status item takes focus before its mouse-up event. If that just hid
    // our panel, treat this click as the same closing gesture, not a reopen.
    let just_hidden = app
        .state::<platform::WindowHost>()
        .last_hidden
        .lock()
        .is_ok_and(|t| t.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250)));
    if !just_hidden {
        platform::show_details(app, None);
    }
}

pub fn position(app: &tauri::AppHandle, window: &tauri::WebviewWindow, areas: &[Area]) {
    let rect = app
        .tray_by_id("pulse-tray")
        .and_then(|tray| tray.rect().ok().flatten());
    let rect = rect.map(|r| {
        let scale = window.scale_factor().unwrap_or(1.);
        let p = r.position.to_physical::<f64>(scale);
        let s = r.size.to_physical::<f64>(scale);
        (p.x, p.y, s.width, s.height)
    });
    let area = rect
        .and_then(|(x, y, width, height)| {
            // A menu bar sits just above the monitor's work area. Match horizontally
            // and choose the vertically closest area (including stacked displays).
            areas
                .iter()
                .filter(|a| {
                    x + width / 2. >= a.x as f64 && x + width / 2. < a.x as f64 + a.width as f64
                })
                .min_by(|a, b| {
                    (a.y as f64 - y - height)
                        .abs()
                        .total_cmp(&(b.y as f64 - y - height).abs())
                })
        })
        .or_else(|| areas.first());
    if let Some(area) = area {
        let (cx, bottom) = rect
            .map(|(x, y, w, h)| (x + w / 2., y + h))
            .unwrap_or((area.x as f64 + area.width as f64, area.y as f64));
        let p = panel_placement(area, cx, bottom);
        let _ = window.set_min_size(Some(tauri::PhysicalSize::new(
            p.width.min(320),
            p.height.min(240),
        )));
        let _ = window.set_size(tauri::PhysicalSize::new(p.width, p.height));
        let _ = window.set_position(tauri::PhysicalPosition::new(p.x, p.y));
    }
}

fn panel_placement(area: &Area, center_x: f64, icon_bottom: f64) -> crate::geometry::Placement {
    let mut p = area.place(None, (380., 800.));
    let max_x = area.x as f64 + area.width as f64 - p.width as f64;
    let max_y = area.y as f64 + area.height as f64 - p.height as f64;
    p.x = (center_x - p.width as f64 / 2.)
        .round()
        .clamp(area.x as f64, max_x) as i32;
    p.y = (icon_bottom + 6. * area.scale)
        .round()
        .clamp(area.y as f64, max_y) as i32;
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_tracks_retina_status_item_and_stays_in_small_work_area() {
        let area = Area {
            monitor: None,
            x: -2880,
            y: 52,
            width: 2880,
            height: 1748,
            scale: 2.,
        };
        let p = panel_placement(&area, -120., 48.);
        assert_eq!((p.width, p.height), (760, 1600));
        assert_eq!(p.x, -760);
        assert_eq!(p.y, 60);
        let area = Area {
            width: 600,
            height: 400,
            ..area
        };
        let p = panel_placement(&area, -2870., 48.);
        assert_eq!((p.x, p.y, p.width, p.height), (-2880, 52, 600, 400));
    }
}

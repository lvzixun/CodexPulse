use tauri::Manager;
use windows_sys::Win32::{
    Foundation::GlobalFree,
    System::{DataExchange::*, Memory::*},
};

const FAILURE: &str = "无法复制诊断信息，请手动选择文本复制";
struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
struct Memory(*mut std::ffi::c_void);
impl Drop for Memory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                GlobalFree(self.0);
            }
        }
    }
}
pub fn copy(app: &tauri::AppHandle, text: &str) -> Result<(), String> {
    if text.len() > 256 * 1024 {
        return Err(FAILURE.into());
    }
    let window = app.get_webview_window("pulse").ok_or(FAILURE)?;
    let owner = window.hwnd().map_err(|_| FAILURE)?;
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut memory = Memory(GlobalAlloc(GMEM_MOVEABLE, units.len() * 2));
        if memory.0.is_null() {
            return Err(FAILURE.into());
        }
        let pointer = GlobalLock(memory.0);
        if pointer.is_null() {
            return Err(FAILURE.into());
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), pointer.cast::<u16>(), units.len());
        GlobalUnlock(memory.0);
        if OpenClipboard(owner.0) == 0 {
            return Err(FAILURE.into());
        }
        let _clipboard = Clipboard;
        if EmptyClipboard() == 0 || SetClipboardData(13, memory.0).is_null() {
            return Err(FAILURE.into());
        }
        memory.0 = std::ptr::null_mut(); // The system owns CF_UNICODETEXT after success.
    }
    Ok(())
}

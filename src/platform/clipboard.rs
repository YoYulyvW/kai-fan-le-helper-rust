//! 剪贴板读写（Win32）。
//! 本模块集中剪贴板相关的 unsafe Win32 调用。
//! 注意：windows-sys 0.52 中部分句柄类型为 isize，用 0 表示空。

#[cfg(windows)]
pub fn get_text() -> String {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return String::new();
        }
        if OpenClipboard(0) == 0 {
            return String::new();
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        let mut result = String::new();
        if handle != 0 {
            // GetClipboardData 返回 isize，GlobalLock 需要 *mut c_void
            let hmem = handle as *mut std::ffi::c_void;
            let ptr = GlobalLock(hmem) as *const u16;
            if !ptr.is_null() {
                let mut len = 0usize;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                result = String::from_utf16_lossy(slice);
                GlobalUnlock(hmem);
            }
        }
        CloseClipboard();
        result
    }
}

/// 写入剪贴板文本
#[cfg(windows)]
pub fn set_text(text: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };

    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if OpenClipboard(0) == 0 {
            return false;
        }
        EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if hmem.is_null() {
            CloseClipboard();
            return false;
        }
        let dst = GlobalLock(hmem) as *mut u16;
        if dst.is_null() {
            CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        GlobalUnlock(hmem);
        // SetClipboardData 需要 isize 句柄
        SetClipboardData(CF_UNICODETEXT, hmem as isize);
        CloseClipboard();
        true
    }
}

/// 清空剪贴板
#[cfg(windows)]
pub fn clear() {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard};
    unsafe {
        if OpenClipboard(0) != 0 {
            EmptyClipboard();
            CloseClipboard();
        }
    }
}

#[cfg(not(windows))]
pub fn get_text() -> String {
    String::new()
}

#[cfg(not(windows))]
pub fn set_text(_text: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn clear() {}

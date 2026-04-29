//! Win32 clipboard — read/write text via OpenClipboard/GetClipboardData/SetClipboardData

extern "C" {
    fn js_nanbox_string(ptr: i64) -> f64;
}

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::DataExchange::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Read text from the system clipboard. Returns NaN-boxed string or TAG_UNDEFINED.
pub fn read() -> f64 {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            if OpenClipboard(None).is_err() {
                return f64::from_bits(0x7FFC_0000_0000_0001); // TAG_UNDEFINED
            }

            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32);
            if handle.is_err() {
                let _ = CloseClipboard();
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }
            let handle = handle.unwrap();

            let ptr = GlobalLock(HGLOBAL(handle.0)) as *const u16;
            if ptr.is_null() {
                let _ = CloseClipboard();
                return f64::from_bits(0x7FFC_0000_0000_0001);
            }

            // Find null terminator
            let mut len = 0;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let wide = std::slice::from_raw_parts(ptr, len);
            let text = String::from_utf16_lossy(wide);

            let _ = GlobalUnlock(HGLOBAL(handle.0));
            let _ = CloseClipboard();

            // Create a Perry string
            let bytes = text.as_bytes();
            let str_ptr =
                perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            js_nanbox_string(str_ptr as i64)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    }
}

/// Write text to the system clipboard.
pub fn write(text_ptr: *const u8) {
    let text = str_from_header(text_ptr);

    #[cfg(target_os = "windows")]
    {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_count = wide.len() * 2;

        unsafe {
            if OpenClipboard(None).is_err() {
                return;
            }
            let _ = EmptyClipboard();

            let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_count);
            if hmem.is_err() {
                let _ = CloseClipboard();
                return;
            }
            let hmem = hmem.unwrap();

            let dst = GlobalLock(hmem) as *mut u16;
            if !dst.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                let _ = GlobalUnlock(hmem);
            }

            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hmem.0));
            let _ = CloseClipboard();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
    }
}

//! Screenshot capture for Windows (behind geisterhand feature).
//!
//! Uses PrintWindow + GDI to capture window, with inline minimal PNG encoder.

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// Capture the main application window as PNG bytes.
/// Returns a malloc'd buffer (caller frees with libc::free). Sets *out_len to byte count.
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    #[cfg(not(target_os = "windows"))]
    {
        return std::ptr::null_mut();
    }

    #[cfg(target_os = "windows")]
    {
        let hwnd = match crate::app::get_main_hwnd() {
            Some(h) => h,
            None => return std::ptr::null_mut(),
        };

        unsafe {
            // Get window dimensions
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return std::ptr::null_mut();
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if width <= 0 || height <= 0 {
                return std::ptr::null_mut();
            }

            // Create a memory DC and compatible bitmap
            let hdc_window = GetDC(Some(hwnd));
            if hdc_window.is_invalid() {
                return std::ptr::null_mut();
            }
            let hdc_mem = CreateCompatibleDC(Some(hdc_window));
            if hdc_mem.is_invalid() {
                ReleaseDC(Some(hwnd), hdc_window);
                return std::ptr::null_mut();
            }
            let hbm = CreateCompatibleBitmap(hdc_window, width, height);
            if hbm.is_invalid() {
                DeleteDC(hdc_mem);
                ReleaseDC(Some(hwnd), hdc_window);
                return std::ptr::null_mut();
            }
            let old_bm = SelectObject(hdc_mem, hbm.into());

            // Capture the window using PrintWindow (PW_RENDERFULLCONTENT = 2)
            let _ = PrintWindow(hwnd, hdc_mem, PRINT_WINDOW_FLAGS(2));

            // Set up BITMAPINFOHEADER for 32-bit BGRA
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let row_bytes = (width as usize) * 4;
            let pixel_data_len = row_bytes * (height as usize);
            let mut pixels = vec![0u8; pixel_data_len];

            let lines = GetDIBits(
                hdc_mem,
                hbm,
                0,
                height as u32,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );

            // Cleanup GDI
            SelectObject(hdc_mem, old_bm);
            let _ = DeleteObject(hbm.into());
            DeleteDC(hdc_mem);
            ReleaseDC(Some(hwnd), hdc_window);

            if lines == 0 {
                return std::ptr::null_mut();
            }

            // Convert BGRA -> RGBA
            for i in (0..pixel_data_len).step_by(4) {
                pixels.swap(i, i + 2); // B <-> R
            }

            // Encode as PNG via the `png` crate (real deflate compression).
            // The old inline encoder wrote stored blocks and produced ~2.7 MB
            // for a 900x788 capture; properly-compressed output is ~50 KB.
            let encoded = encode_png_rgba(width as u32, height as u32, &pixels);

            let len = encoded.len();
            let buf = libc::malloc(len) as *mut u8;
            if buf.is_null() {
                return std::ptr::null_mut();
            }
            std::ptr::copy_nonoverlapping(encoded.as_ptr(), buf, len);
            *out_len = len;
            buf
        }
    }
}

// ---------------------------------------------------------------------------
// PNG encoder — uses the `png` crate for real deflate compression.
// ---------------------------------------------------------------------------

/// Encode RGBA pixel data as a compressed PNG file.
#[cfg(target_os = "windows")]
fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4);
    let ok = {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // Fast compression — deflate level 1 is ~30x faster than level 9
        // and typical gallery screenshots still land under 100 KB.
        encoder.set_compression(png::Compression::Fast);
        // Keep the encoder borrow strictly inside this block so we can
        // freely return `out` below.
        match encoder.write_header() {
            Ok(mut writer) => writer.write_image_data(rgba).is_ok(),
            Err(_) => false,
        }
    };
    if !ok {
        out.clear();
    }
    out
}

// (write_chunk / crc32 / adler32 helpers removed — the `png` crate
//  handles chunk framing, CRC32, and Adler-32 internally.)

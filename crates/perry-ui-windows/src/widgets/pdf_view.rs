//! PdfView widget backed by `Windows.Data.Pdf`.
//!
//! PDF documents are decoded by the Windows Runtime and their current page is
//! cached as PNG bytes. A subclassed Win32 STATIC control paints that bitmap
//! with GDI+, preserving the page aspect ratio. Loading and page rendering run
//! on MTA worker threads: `load_file` waits for the initial page because its FFI
//! contract reports parse success synchronously, while subsequent navigation
//! and zoom renders post their result back to the widget without blocking the
//! UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStrExt;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "windows")]
use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "windows")]
use windows::core::HSTRING;
#[cfg(target_os = "windows")]
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
#[cfg(target_os = "windows")]
use windows::Storage::StorageFile;
#[cfg(target_os = "windows")]
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::InvalidateRect;
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};
use perry_ffi::copy_string_from_raw as str_from_header;

#[cfg(target_os = "windows")]
const PDF_SUBCLASS_ID: usize = 0x70_64_66_76; // "pdfv"
#[cfg(target_os = "windows")]
const PDF_RENDERED_MSG: u32 = 0x0400 + 0x502;
#[cfg(target_os = "windows")]
const MAX_RENDER_DIMENSION: u32 = 8192;

struct PdfState {
    path: Option<PathBuf>,
    page_count: i64,
    current_page: i64,
    scale: f64,
    rendered_png: Option<Arc<[u8]>>,
    render_error: Option<String>,
    #[cfg(target_os = "windows")]
    document: Option<PdfDocument>,
    #[cfg(target_os = "windows")]
    pending_request: usize,
}

thread_local! {
    static PDFS: RefCell<HashMap<i64, PdfState>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "windows")]
struct LoadedPdf {
    path: PathBuf,
    document: PdfDocument,
    page_count: i64,
    first_page: Result<Vec<u8>, String>,
}

#[cfg(target_os = "windows")]
struct RenderCompletion {
    handle: i64,
    result: Result<Vec<u8>, String>,
}

#[cfg(target_os = "windows")]
fn completed_renders() -> &'static Mutex<HashMap<usize, RenderCompletion>> {
    static COMPLETED: OnceLock<Mutex<HashMap<usize, RenderCompletion>>> = OnceLock::new();
    COMPLETED.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(target_os = "windows")]
fn next_render_request() -> usize {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    let request = NEXT.fetch_add(1, Ordering::Relaxed);
    if request == 0 {
        NEXT.fetch_add(1, Ordering::Relaxed)
    } else {
        request
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn normalized_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.max(0.1)
    } else {
        1.0
    }
}

pub fn create(width: f64, height: f64) -> i64 {
    let control_id = alloc_control_id();
    let w = if width > 0.0 { width as i32 } else { 600 };
    let h = if height > 0.0 { height as i32 } else { 400 };

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0),
                0,
                0,
                w,
                h,
                Some(super::get_parking_hwnd()),
                Some(HMENU(control_id as *mut _)),
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);
            PDFS.with(|pdfs| {
                pdfs.borrow_mut().insert(
                    handle,
                    PdfState {
                        path: None,
                        page_count: 0,
                        current_page: -1,
                        scale: 1.0,
                        rendered_png: None,
                        render_error: None,
                        document: None,
                        pending_request: 0,
                    },
                );
            });
            let _ = SetWindowSubclass(
                hwnd,
                Some(pdf_subclass_proc),
                PDF_SUBCLASS_ID,
                handle as usize,
            );
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (w, h);
        let handle = register_widget(0, WidgetKind::Image, control_id);
        PDFS.with(|pdfs| {
            pdfs.borrow_mut().insert(
                handle,
                PdfState {
                    path: None,
                    page_count: 0,
                    current_page: -1,
                    scale: 1.0,
                    rendered_png: None,
                    render_error: None,
                },
            );
        });
        handle
    }
}

/// Load a PDF file. Returns 1 only when Windows successfully parses the
/// document. The first page is rendered before returning so a successful load
/// never leaves the old placeholder label behind.
pub fn load_file(handle: i64, path_ptr: *const u8) -> i64 {
    let valid_handle = PDFS.with(|pdfs| pdfs.borrow().contains_key(&handle));
    if !valid_handle {
        return 0;
    }
    let path_string = unsafe { str_from_header(path_ptr) };
    if path_string.is_empty() {
        set_load_error(handle, None, "PDF path is empty".to_string());
        return 0;
    }
    let path = PathBuf::from(path_string);

    #[cfg(target_os = "windows")]
    {
        let scale = PDFS.with(|pdfs| {
            pdfs.borrow()
                .get(&handle)
                .map(|state| state.scale)
                .unwrap_or(1.0)
        });
        match load_pdf_on_worker(path.clone(), scale) {
            Ok(loaded) => {
                let first_page_error = loaded.first_page.as_ref().err().cloned();
                PDFS.with(|pdfs| {
                    if let Some(state) = pdfs.borrow_mut().get_mut(&handle) {
                        state.path = Some(loaded.path);
                        state.page_count = loaded.page_count;
                        state.current_page = 0;
                        state.rendered_png = loaded.first_page.ok().map(Arc::<[u8]>::from);
                        state.render_error = first_page_error.clone();
                        state.document = Some(loaded.document);
                        state.pending_request = 0;
                    }
                });
                if let Some(error) = first_page_error {
                    eprintln!("[perry-ui-windows] PdfView page render failed: {error}");
                }
                invalidate(handle);
                1
            }
            Err(error) => {
                eprintln!("[perry-ui-windows] PdfView load failed: {error}");
                set_load_error(handle, Some(path), error);
                0
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        set_load_error(
            handle,
            None,
            "PDF rendering is unavailable on this target".to_string(),
        );
        0
    }
}

fn set_load_error(handle: i64, path: Option<PathBuf>, error: String) {
    PDFS.with(|pdfs| {
        if let Some(state) = pdfs.borrow_mut().get_mut(&handle) {
            state.path = path;
            state.page_count = 0;
            state.current_page = -1;
            state.rendered_png = None;
            state.render_error = Some(error);
            #[cfg(target_os = "windows")]
            {
                state.document = None;
                state.pending_request = 0;
            }
        }
    });
    #[cfg(target_os = "windows")]
    invalidate(handle);
}

pub fn get_page_count(handle: i64) -> i64 {
    PDFS.with(|pdfs| {
        pdfs.borrow()
            .get(&handle)
            .map(|state| state.page_count)
            .unwrap_or(0)
    })
}

/// Jump to a zero-based page. Invalid page indexes are ignored, matching the
/// PDFKit backends.
pub fn go_to_page(handle: i64, page_index: i64) {
    let changed = PDFS.with(|pdfs| {
        let mut pdfs = pdfs.borrow_mut();
        let Some(state) = pdfs.get_mut(&handle) else {
            return false;
        };
        if page_index < 0 || page_index >= state.page_count || page_index == state.current_page {
            return false;
        }
        state.current_page = page_index;
        true
    });

    #[cfg(target_os = "windows")]
    if changed {
        schedule_render(handle);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = changed;
}

pub fn get_current_page(handle: i64) -> i64 {
    PDFS.with(|pdfs| {
        pdfs.borrow()
            .get(&handle)
            .map(|state| state.current_page)
            .unwrap_or(-1)
    })
}

pub fn set_scale(handle: i64, scale: f64) {
    let scale = normalized_scale(scale);
    let changed = PDFS.with(|pdfs| {
        let mut pdfs = pdfs.borrow_mut();
        let Some(state) = pdfs.get_mut(&handle) else {
            return false;
        };
        if (state.scale - scale).abs() < f64::EPSILON {
            return false;
        }
        state.scale = scale;
        state.page_count > 0
    });

    #[cfg(target_os = "windows")]
    if changed {
        // Repaint immediately at the new logical zoom using the cached page,
        // then replace it with a resolution-matched render when ready.
        invalidate(handle);
        schedule_render(handle);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = changed;
}

#[cfg(target_os = "windows")]
fn invalidate(handle: i64) {
    if let Some(hwnd) = super::get_hwnd(handle) {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}

#[cfg(target_os = "windows")]
fn load_pdf_on_worker(path: PathBuf, scale: f64) -> Result<LoadedPdf, String> {
    std::thread::spawn(move || {
        initialize_mta()?;
        let canonical =
            std::fs::canonicalize(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let hpath = storage_path_hstring(&canonical);
        let file = StorageFile::GetFileFromPathAsync(&hpath)
            .map_err(|error| format!("StorageFile lookup failed: {error}"))?
            .join()
            .map_err(|error| format!("StorageFile lookup failed: {error}"))?;
        let document = PdfDocument::LoadFromFileAsync(&file)
            .map_err(|error| format!("PDF decode failed: {error}"))?
            .join()
            .map_err(|error| format!("PDF decode failed: {error}"))?;
        let page_count = document
            .PageCount()
            .map_err(|error| format!("could not read PDF page count: {error}"))?
            as i64;
        if page_count == 0 {
            return Err("the PDF contains no pages".to_string());
        }
        let first_page = render_page_png(&document, 0, scale);
        Ok(LoadedPdf {
            path: canonical,
            document,
            page_count,
            first_page,
        })
    })
    .join()
    .map_err(|_| "PDF loader worker panicked".to_string())?
}

#[cfg(target_os = "windows")]
fn initialize_mta() -> Result<(), String> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
        .ok()
        .map_err(|error| format!("could not initialize Windows Runtime apartment: {error}"))
}

/// `canonicalize` commonly adds the Win32 verbatim prefix. StorageFile wants a
/// normal drive or UNC path, so remove only that prefix while retaining the
/// original UTF-16 path losslessly.
#[cfg(target_os = "windows")]
fn storage_path_hstring(path: &Path) -> HSTRING {
    const VERBATIM: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if let Some(rest) = wide.strip_prefix(VERBATIM_UNC) {
        let mut normalized = vec![b'\\' as u16, b'\\' as u16];
        normalized.extend_from_slice(rest);
        HSTRING::from_wide(&normalized)
    } else if let Some(rest) = wide.strip_prefix(VERBATIM) {
        HSTRING::from_wide(rest)
    } else {
        HSTRING::from_wide(&wide)
    }
}

#[cfg(target_os = "windows")]
fn schedule_render(handle: i64) {
    let request = next_render_request();
    let job = PDFS.with(|pdfs| {
        let mut pdfs = pdfs.borrow_mut();
        let state = pdfs.get_mut(&handle)?;
        let document = state.document.clone()?;
        let hwnd = super::get_hwnd(handle)?;
        state.pending_request = request;
        state.render_error = None;
        Some((
            document,
            state.current_page as u32,
            state.scale,
            hwnd.0 as isize,
        ))
    });
    let Some((document, page_index, scale, hwnd_value)) = job else {
        return;
    };

    std::thread::spawn(move || {
        let result = initialize_mta().and_then(|()| render_page_png(&document, page_index, scale));
        if let Ok(mut completed) = completed_renders().lock() {
            completed.insert(request, RenderCompletion { handle, result });
        } else {
            return;
        }
        let hwnd = HWND(hwnd_value as *mut _);
        let posted =
            unsafe { PostMessageW(Some(hwnd), PDF_RENDERED_MSG, WPARAM(request), LPARAM(0)) };
        if posted.is_err() {
            if let Ok(mut completed) = completed_renders().lock() {
                completed.remove(&request);
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn render_page_png(document: &PdfDocument, page_index: u32, scale: f64) -> Result<Vec<u8>, String> {
    let page = document
        .GetPage(page_index)
        .map_err(|error| format!("could not open page {}: {error}", page_index + 1))?;
    let size = page
        .Size()
        .map_err(|error| format!("could not read page dimensions: {error}"))?;
    let (width, height) = render_dimensions(size.Width, size.Height, scale);
    let options = PdfPageRenderOptions::new()
        .map_err(|error| format!("could not create PDF render options: {error}"))?;
    options
        .SetDestinationWidth(width)
        .map_err(|error| format!("could not set PDF render width: {error}"))?;
    options
        .SetDestinationHeight(height)
        .map_err(|error| format!("could not set PDF render height: {error}"))?;

    let stream = InMemoryRandomAccessStream::new()
        .map_err(|error| format!("could not create PDF render stream: {error}"))?;
    page.RenderWithOptionsToStreamAsync(&stream, &options)
        .map_err(|error| format!("could not start PDF page render: {error}"))?
        .join()
        .map_err(|error| format!("PDF page render failed: {error}"))?;

    let size = stream
        .Size()
        .map_err(|error| format!("could not read rendered page size: {error}"))?;
    let byte_count = u32::try_from(size)
        .map_err(|_| format!("rendered PDF page is too large ({size} bytes)"))?;
    if byte_count == 0 {
        return Err("PDF renderer returned an empty image".to_string());
    }
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|error| format!("could not rewind rendered PDF page: {error}"))?;
    let reader = DataReader::CreateDataReader(&input)
        .map_err(|error| format!("could not create PDF image reader: {error}"))?;
    let loaded = reader
        .LoadAsync(byte_count)
        .map_err(|error| format!("could not start reading PDF image: {error}"))?
        .join()
        .map_err(|error| format!("could not read PDF image: {error}"))?;
    if loaded != byte_count {
        return Err(format!(
            "PDF image was truncated (expected {byte_count} bytes, got {loaded})"
        ));
    }
    let mut bytes = vec![0u8; byte_count as usize];
    reader
        .ReadBytes(&mut bytes)
        .map_err(|error| format!("could not copy PDF image bytes: {error}"))?;
    let _ = reader.Close();
    let _ = stream.Close();
    let _ = page.Close();
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn render_dimensions(width: f32, height: f32, scale: f64) -> (u32, u32) {
    let width = if width.is_finite() && width > 0.0 {
        f64::from(width)
    } else {
        1.0
    };
    let height = if height.is_finite() && height > 0.0 {
        f64::from(height)
    } else {
        1.0
    };
    let scale = normalized_scale(scale);
    let longest = width.max(height);
    let capped_scale = scale.min(f64::from(MAX_RENDER_DIMENSION) / longest);
    let rendered_width = (width * capped_scale)
        .round()
        .clamp(1.0, f64::from(MAX_RENDER_DIMENSION));
    let rendered_height = (height * capped_scale)
        .round()
        .clamp(1.0, f64::from(MAX_RENDER_DIMENSION));
    (rendered_width as u32, rendered_height as u32)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn pdf_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    let handle = refdata as i64;
    match msg {
        WM_PAINT => {
            paint_pdf(hwnd, handle);
            return LRESULT(0);
        }
        WM_ERASEBKGND => return LRESULT(1),
        PDF_RENDERED_MSG => {
            accept_render_completion(hwnd, handle, wparam.0);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            let pending = PDFS.with(|pdfs| {
                pdfs.borrow_mut()
                    .remove(&handle)
                    .map(|state| state.pending_request)
                    .unwrap_or(0)
            });
            if pending != 0 {
                if let Ok(mut completed) = completed_renders().lock() {
                    completed.remove(&pending);
                }
            }
            let _ = RemoveWindowSubclass(hwnd, Some(pdf_subclass_proc), PDF_SUBCLASS_ID);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(target_os = "windows")]
unsafe fn accept_render_completion(hwnd: HWND, handle: i64, request: usize) {
    let completion = completed_renders()
        .lock()
        .ok()
        .and_then(|mut completed| completed.remove(&request));
    let Some(completion) = completion else {
        return;
    };
    if completion.handle != handle {
        return;
    }

    let mut error_to_log = None;
    let accepted = PDFS.with(|pdfs| {
        let mut pdfs = pdfs.borrow_mut();
        let Some(state) = pdfs.get_mut(&handle) else {
            return false;
        };
        if state.pending_request != request {
            return false;
        }
        state.pending_request = 0;
        match completion.result {
            Ok(bytes) => {
                state.rendered_png = Some(Arc::<[u8]>::from(bytes));
                state.render_error = None;
            }
            Err(error) => {
                error_to_log = Some(error.clone());
                state.rendered_png = None;
                state.render_error = Some(error);
            }
        }
        true
    });
    if let Some(error) = error_to_log {
        eprintln!("[perry-ui-windows] PdfView page render failed: {error}");
    }
    if accepted {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

#[cfg(target_os = "windows")]
unsafe fn paint_pdf(hwnd: HWND, handle: i64) {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::Graphics::GdiPlus::*;
    use windows::Win32::UI::Shell::SHCreateMemStream;

    let (png, scale, has_error) = PDFS.with(|pdfs| {
        pdfs.borrow()
            .get(&handle)
            .map_or((None, 1.0, false), |state| {
                (
                    state.rendered_png.clone(),
                    state.scale,
                    state.render_error.is_some(),
                )
            })
    });

    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    if hdc.is_invalid() {
        return;
    }
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let background = CreateSolidBrush(COLORREF(0x00_FF_FF_FF));
    FillRect(hdc, &client, background);
    let _ = DeleteObject(background.into());

    if let Some(png) = png {
        if let Some(stream) = SHCreateMemStream(Some(png.as_ref())) {
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                ..Default::default()
            };
            let mut token = 0usize;
            if GdiplusStartup(&mut token, &input, std::ptr::null_mut()).0 == 0 {
                let mut image: *mut GpImage = std::ptr::null_mut();
                if GdipLoadImageFromStream(&stream, &mut image).0 == 0 && !image.is_null() {
                    let mut image_width = 0u32;
                    let mut image_height = 0u32;
                    let _ = GdipGetImageWidth(image, &mut image_width);
                    let _ = GdipGetImageHeight(image, &mut image_height);
                    if image_width > 0 && image_height > 0 {
                        let client_width = (client.right - client.left).max(1) as f64;
                        let client_height = (client.bottom - client.top).max(1) as f64;
                        let fit = (client_width / f64::from(image_width))
                            .min(client_height / f64::from(image_height));
                        let draw_width = (f64::from(image_width) * fit * scale)
                            .round()
                            .clamp(1.0, f64::from(i32::MAX))
                            as i32;
                        let draw_height = (f64::from(image_height) * fit * scale)
                            .round()
                            .clamp(1.0, f64::from(i32::MAX))
                            as i32;
                        let x = (client.right - client.left - draw_width) / 2;
                        let y = (client.bottom - client.top - draw_height) / 2;
                        let mut graphics: *mut GpGraphics = std::ptr::null_mut();
                        let _ = GdipCreateFromHDC(hdc, &mut graphics);
                        if !graphics.is_null() {
                            let _ = GdipSetInterpolationMode(graphics, InterpolationMode(7));
                            let _ =
                                GdipDrawImageRectI(graphics, image, x, y, draw_width, draw_height);
                            let _ = GdipDeleteGraphics(graphics);
                        }
                    }
                    let _ = GdipDisposeImage(image);
                }
                GdiplusShutdown(token);
            }
        }
    } else if has_error {
        let mut message = to_wide("Unable to display this PDF page.");
        let text_len = message.len().saturating_sub(1);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLORREF(0x00_55_55_55));
        let _ = DrawTextW(
            hdc,
            &mut message[..text_len],
            &mut client,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
    }

    let _ = EndPaint(hwnd, &paint);
}

#[cfg(test)]
mod tests {
    use super::normalized_scale;

    #[test]
    fn pdf_scale_is_finite_and_has_safe_minimum() {
        assert_eq!(normalized_scale(0.01), 0.1);
        assert_eq!(normalized_scale(2.5), 2.5);
        assert_eq!(normalized_scale(100.0), 100.0);
        assert_eq!(normalized_scale(f64::NAN), 1.0);
        assert_eq!(normalized_scale(f64::INFINITY), 1.0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn render_size_preserves_page_aspect_ratio_and_caps_allocation() {
        assert_eq!(super::render_dimensions(612.0, 792.0, 1.0), (612, 792));
        assert_eq!(super::render_dimensions(612.0, 792.0, 2.0), (1224, 1584));
        let (width, height) = super::render_dimensions(612.0, 792.0, 16.0);
        assert_eq!(height, super::MAX_RENDER_DIMENSION);
        assert!((f64::from(width) / f64::from(height) - 612.0 / 792.0).abs() < 0.001);
        assert_eq!(
            super::render_dimensions(f32::NAN, f32::INFINITY, 1.0),
            (1, 1)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_pdf_backend_decodes_and_renders_a_real_page() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_FILE: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "perry-pdf-view-{}-{}.pdf",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, one_page_pdf()).expect("write PDF fixture");

        let loaded = super::load_pdf_on_worker(path.clone(), 1.0).expect("load PDF through WinRT");
        let image = loaded.first_page.expect("render first PDF page");
        assert_eq!(loaded.page_count, 1);
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");

        let _ = std::fs::remove_file(path);
    }

    #[cfg(target_os = "windows")]
    fn one_page_pdf() -> Vec<u8> {
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 4 0 R >>".as_slice(),
            b"<< /Length 0 >>\nstream\n\nendstream".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(objects.len());
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }
}

//! Native bindings for the npm `sharp` image-processing package —
//! uses only perry-ffi. Sync transforms (resize / rotate / flip /
//! grayscale / blur / sharpen / crop / format selectors) plus three
//! async exports (`toFile` / `toBuffer` / `metadata`) bridged
//! through `spawn_blocking` + `JsPromise`.

use base64::Engine;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use perry_ffi::{
    alloc_string, get_handle, read_bytes, read_string, register_handle, spawn_blocking, Handle,
    JsPromise, JsString, Promise, StringHeader,
};
use std::io::Cursor;

pub struct SharpHandle {
    pub image: DynamicImage,
    pub format: ImageFormat,
    pub quality: u8,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

unsafe fn read_buf(ptr: *const StringHeader) -> Option<Vec<u8>> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_bytes(handle).map(|b| b.to_vec())
}

fn fmt_name(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        ImageFormat::Gif => "gif",
        _ => "unknown",
    }
}

/// # Safety
/// `path_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_from_file(path_ptr: *const StringHeader) -> Handle {
    let path = match read_str(path_ptr) {
        Some(p) => p,
        None => return -1,
    };
    match image::open(&path) {
        Ok(img) => {
            let format = ImageFormat::from_path(&path).unwrap_or(ImageFormat::Png);
            register_handle(SharpHandle {
                image: img,
                format,
                quality: 80,
            })
        }
        Err(_) => -1,
    }
}

/// # Safety
/// `buffer_ptr` must be null or a Perry-runtime `StringHeader`
/// (binary bytes — UTF-8 not required).
#[no_mangle]
pub unsafe extern "C" fn js_sharp_from_buffer(buffer_ptr: *const StringHeader) -> Handle {
    let buffer = match read_buf(buffer_ptr) {
        Some(b) => b,
        None => return -1,
    };
    match image::load_from_memory(&buffer) {
        Ok(img) => {
            let format = image::guess_format(&buffer).unwrap_or(ImageFormat::Png);
            register_handle(SharpHandle {
                image: img,
                format,
                quality: 80,
            })
        }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn js_sharp_resize(handle: Handle, width: f64, height: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let new_width = width as u32;
        let new_height = if height > 0.0 {
            height as u32
        } else {
            let (orig_w, orig_h) = sharp.image.dimensions();
            (new_width as f64 * orig_h as f64 / orig_w as f64) as u32
        };
        let resized = sharp
            .image
            .resize(new_width, new_height, FilterType::Lanczos3);
        return register_handle(SharpHandle {
            image: resized,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_rotate(handle: Handle, angle: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let rotated = match angle as i32 {
            90 => sharp.image.rotate90(),
            180 => sharp.image.rotate180(),
            270 => sharp.image.rotate270(),
            _ => sharp.image.clone(),
        };
        return register_handle(SharpHandle {
            image: rotated,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_flip(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.flipv(),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_flop(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.fliph(),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_grayscale(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.grayscale(),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_blur(handle: Handle, sigma: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.blur(sigma as f32),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_sharpen(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.unsharpen(1.0, 1),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_crop(
    handle: Handle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp
                .image
                .crop_imm(left as u32, top as u32, width as u32, height as u32),
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_jpeg(handle: Handle, quality: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::Jpeg,
            quality: if quality > 0.0 { quality as u8 } else { 80 },
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_png(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::Png,
            quality: sharp.quality,
        });
    }
    -1
}

#[no_mangle]
pub extern "C" fn js_sharp_webp(handle: Handle, quality: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::WebP,
            quality: if quality > 0.0 { quality as u8 } else { 80 },
        });
    }
    -1
}

/// # Safety
/// `path_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_to_file(
    handle: Handle,
    path_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    let path = match read_str(path_ptr) {
        Some(p) => p,
        None => {
            promise.reject_string("Invalid path");
            return raw;
        }
    };

    spawn_blocking(move || {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            match sharp.image.save(&path) {
                Ok(_) => {
                    let (width, height) = sharp.image.dimensions();
                    let info = format!(
                        r#"{{"width":{},"height":{},"format":"{}"}}"#,
                        width,
                        height,
                        fmt_name(sharp.format)
                    );
                    promise.resolve_string(&info);
                }
                Err(e) => promise.reject_string(&format!("Failed to save image: {}", e)),
            }
        } else {
            promise.reject_string("Invalid sharp handle");
        }
    });
    raw
}

#[no_mangle]
pub extern "C" fn js_sharp_to_buffer(handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            let mut buffer = Cursor::new(Vec::new());
            match sharp.image.write_to(&mut buffer, sharp.format) {
                Ok(_) => {
                    let bytes = buffer.into_inner();
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    promise.resolve_string(&encoded);
                }
                Err(e) => promise.reject_string(&format!("Failed to encode image: {}", e)),
            }
        } else {
            promise.reject_string("Invalid sharp handle");
        }
    });
    raw
}

#[no_mangle]
pub extern "C" fn js_sharp_metadata(handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    spawn_blocking(move || {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            let (width, height) = sharp.image.dimensions();
            let channels = sharp.image.color().channel_count();
            let info = format!(
                r#"{{"width":{},"height":{},"channels":{},"format":"{}"}}"#,
                width,
                height,
                channels,
                fmt_name(sharp.format)
            );
            promise.resolve_string(&info);
        } else {
            promise.reject_string("Invalid sharp handle");
        }
    });
    raw
}

#[no_mangle]
pub extern "C" fn js_sharp_width(handle: Handle) -> f64 {
    get_handle::<SharpHandle>(handle)
        .map(|s| s.image.width() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_sharp_height(handle: Handle) -> f64 {
    get_handle::<SharpHandle>(handle)
        .map(|s| s.image.height() as f64)
        .unwrap_or(0.0)
}

// `alloc_string` is currently unused — kept available for follow-ups
// that may need to surface error messages via JsString returns.
#[allow(dead_code)]
fn _ensure_alloc_string_linkage() -> *mut StringHeader {
    alloc_string("").as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn make_handle(w: u32, h: u32) -> Handle {
        let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(w, h, Rgba([255, 0, 0, 255]));
        let img = DynamicImage::ImageRgba8(buf);
        register_handle(SharpHandle {
            image: img,
            format: ImageFormat::Png,
            quality: 80,
        })
    }

    #[test]
    fn width_height_basic() {
        let h = make_handle(100, 50);
        assert_eq!(js_sharp_width(h), 100.0);
        assert_eq!(js_sharp_height(h), 50.0);
    }

    #[test]
    fn resize_scales() {
        let h = make_handle(100, 50);
        let h2 = js_sharp_resize(h, 200.0, 100.0);
        assert!(h2 >= 0);
        assert_eq!(js_sharp_width(h2), 200.0);
        assert_eq!(js_sharp_height(h2), 100.0);
    }

    #[test]
    fn resize_aspect_ratio_preserved_when_height_zero() {
        let h = make_handle(100, 50);
        let h2 = js_sharp_resize(h, 200.0, 0.0);
        assert_eq!(js_sharp_width(h2), 200.0);
        assert_eq!(js_sharp_height(h2), 100.0);
    }

    #[test]
    fn rotate_90_swaps_dimensions() {
        let h = make_handle(100, 50);
        let h2 = js_sharp_rotate(h, 90.0);
        assert_eq!(js_sharp_width(h2), 50.0);
        assert_eq!(js_sharp_height(h2), 100.0);
    }

    #[test]
    fn jpeg_sets_format_and_quality() {
        let h = make_handle(10, 10);
        let h2 = js_sharp_jpeg(h, 95.0);
        assert!(get_handle::<SharpHandle>(h2).map(|s| s.quality).unwrap() == 95);
    }

    #[test]
    fn invalid_handle_returns_zero_dims() {
        assert_eq!(js_sharp_width(-1), 0.0);
        assert_eq!(js_sharp_height(-1), 0.0);
    }
}

//! Native bindings for the npm `sharp` image-processing package —
//! uses only perry-ffi. Sync transforms (resize / rotate / flip /
//! grayscale / blur / sharpen / crop / format selectors) plus three
//! async exports (`toFile` / `toBuffer` / `metadata`) bridged
//! through `spawn_blocking` + `JsPromise`.

use image::{DynamicImage, GenericImageView, ImageFormat};
use perry_ffi::{
    alloc_buffer, alloc_string, build_object_shape, get_handle, js_object_alloc_with_shape,
    js_object_set_field, read_buffer_bytes, read_bytes, read_string, register_handle,
    spawn_blocking, BufferHeader, Handle, JsPromise, JsString, JsValue, ObjectHeader, Promise,
    StringHeader,
};
use std::io::Cursor;

// perry-runtime `#[no_mangle]` symbols (always linked) used to inspect raw
// NaN-boxed JS values at the ext-crate boundary: the unified pointer mask
// (works for strings AND buffers/objects), the Buffer-registry probe, and
// the by-name numeric field reader for `.extract({...})` options objects.
extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_buffer_is_buffer(ptr: i64) -> i32;
    fn js_object_get_field_by_name_f64(obj: *const ObjectHeader, key: *const StringHeader) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
}

/// Read a numeric field by name from a NaN-boxed JS options object. Returns
/// `None` if `opts` isn't an object or the field isn't a number. Handles both
/// int32- and f64-boxed numbers.
unsafe fn opts_number_field(opts: f64, name: &str) -> Option<f64> {
    let jv = JsValue::from_bits(opts.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let obj = jv.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        return None;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let field = JsValue::from_bits(js_object_get_field_by_name_f64(obj, key).to_bits());
    if field.is_int32() {
        Some(((field.bits() & 0xFFFF_FFFF) as u32 as i32) as f64)
    } else if field.is_number() {
        Some(f64::from_bits(field.bits()))
    } else {
        None
    }
}

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

fn open_image_path(path: &str) -> Handle {
    match image::open(path) {
        Ok(img) => register_handle(SharpHandle {
            image: img,
            format: ImageFormat::from_path(path).unwrap_or(ImageFormat::Png),
            quality: 80,
        }),
        Err(_) => -1,
    }
}

fn decode_image_bytes(bytes: &[u8]) -> Handle {
    match image::load_from_memory(bytes) {
        Ok(img) => register_handle(SharpHandle {
            image: img,
            format: image::guess_format(bytes).unwrap_or(ImageFormat::Png),
            quality: 80,
        }),
        Err(_) => -1,
    }
}

/// `sharp(input)` factory — `input` is a file path string OR a Buffer /
/// Uint8Array of encoded image bytes. The arg arrives as raw NaN-box bits
/// (NA_JSV); recover the underlying pointer and branch on the Buffer registry
/// probe.
///
/// # Safety
/// `input_bits` must be the raw NaN-box bits of a JS string or Buffer value.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_from_input(input_bits: i64) -> Handle {
    let ptr = js_get_string_pointer_unified(f64::from_bits(input_bits as u64));
    if ptr == 0 {
        return -1;
    }
    if js_buffer_is_buffer(ptr) != 0 {
        return match read_buffer_bytes(ptr as *const BufferHeader) {
            Some(bytes) => decode_image_bytes(bytes),
            None => -1,
        };
    }
    // A POINTER_TAG value that isn't a registered Buffer is a plain object /
    // array — not a valid sharp input. `js_get_string_pointer_unified` hands
    // back its heap pointer, which must NOT be read as a `StringHeader` (that
    // would read arbitrary memory). Reject it the way sharp rejects an
    // unsupported input. (Strings — long or short — and number-coerced keys
    // are not `POINTER_TAG`, so the path-string case still flows through.)
    if JsValue::from_bits(input_bits as u64).is_pointer() {
        return -1;
    }
    match read_string(JsString::from_raw(ptr as *mut StringHeader)) {
        Some(path) => open_image_path(path),
        None => -1,
    }
}

/// SIMD-accelerated Lanczos3 resize via `fast_image_resize`, preserving the
/// source pixel layout (Luma / LumaA / Rgb / Rgba 8-bit). Falls back to the
/// `image` crate's resize for less common encodings (16-bit, float).
fn fast_resize(img: &DynamicImage, dw: u32, dh: u32) -> DynamicImage {
    use fast_image_resize::images::Image;
    use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

    let (sw, sh) = img.dimensions();
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return img.clone();
    }

    // (pixel_type, raw source bytes, rebuild-into-DynamicImage closure)
    let (pixel_type, src_bytes): (PixelType, Vec<u8>) = match img {
        DynamicImage::ImageLuma8(b) => (PixelType::U8, b.as_raw().clone()),
        DynamicImage::ImageLumaA8(b) => (PixelType::U8x2, b.as_raw().clone()),
        DynamicImage::ImageRgb8(b) => (PixelType::U8x3, b.as_raw().clone()),
        DynamicImage::ImageRgba8(b) => (PixelType::U8x4, b.as_raw().clone()),
        // Uncommon (16-bit / float): convert to RGBA8 and resize that.
        other => (PixelType::U8x4, other.to_rgba8().into_raw()),
    };

    let resized = (|| {
        let src = Image::from_vec_u8(sw, sh, src_bytes, pixel_type).ok()?;
        let mut dst = Image::new(dw, dh, pixel_type);
        Resizer::new()
            .resize(
                &src,
                &mut dst,
                &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
            )
            .ok()?;
        let raw = dst.into_vec();
        let dyn_img = match pixel_type {
            PixelType::U8 => DynamicImage::ImageLuma8(image::ImageBuffer::from_raw(dw, dh, raw)?),
            PixelType::U8x2 => {
                DynamicImage::ImageLumaA8(image::ImageBuffer::from_raw(dw, dh, raw)?)
            }
            PixelType::U8x3 => DynamicImage::ImageRgb8(image::ImageBuffer::from_raw(dw, dh, raw)?),
            _ => DynamicImage::ImageRgba8(image::ImageBuffer::from_raw(dw, dh, raw)?),
        };
        Some(dyn_img)
    })();

    // Fall back to the image crate if anything in the fast path failed.
    resized.unwrap_or_else(|| img.resize_exact(dw, dh, image::imageops::FilterType::Lanczos3))
}

#[no_mangle]
pub extern "C" fn js_sharp_resize(handle: Handle, width: f64, height: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let new_width = width as u32;
        let new_height = if height > 0.0 {
            height as u32
        } else {
            let (orig_w, orig_h) = sharp.image.dimensions();
            if orig_w == 0 {
                0
            } else {
                (new_width as f64 * orig_h as f64 / orig_w as f64).round() as u32
            }
        };
        let resized = fast_resize(&sharp.image, new_width, new_height);
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

/// `.extract({ left, top, width, height })` — sharp's region crop. Reads the
/// four numeric fields from the options object; missing/invalid fields
/// default to 0.
///
/// # Safety
/// `opts` carries the raw NaN-boxed bits of a JS object (passed as f64).
#[no_mangle]
pub unsafe extern "C" fn js_sharp_extract(handle: Handle, opts: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let field = |name: &str| opts_number_field(opts, name).unwrap_or(0.0).max(0.0) as u32;
        return register_handle(SharpHandle {
            image: sharp.image.crop_imm(
                field("left"),
                field("top"),
                field("width"),
                field("height"),
            ),
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
                    // sharp's toFile resolves an `info` object, not a string:
                    // `{ format, width, height, channels, size }`.
                    let (width, height) = sharp.image.dimensions();
                    // Report the ENCODED output's channel count by re-reading
                    // the saved file (e.g. an RGBA source saved as JPEG is
                    // 3-channel on disk), falling back to the in-memory count.
                    let channels = image::open(&path)
                        .map(|saved| saved.color().channel_count())
                        .unwrap_or_else(|_| sharp.image.color().channel_count());
                    let format = fmt_name(sharp.format).to_string();
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    promise.resolve_with(move || {
                        let (packed, shape_id) =
                            build_object_shape(&["format", "width", "height", "channels", "size"]);
                        let obj = unsafe {
                            js_object_alloc_with_shape(
                                shape_id,
                                5,
                                packed.as_ptr(),
                                packed.len() as u32,
                            )
                        };
                        unsafe {
                            js_object_set_field(
                                obj,
                                0,
                                JsValue::from_string_ptr(alloc_string(&format).as_raw()),
                            );
                            js_object_set_field(obj, 1, JsValue::from_number(width as f64));
                            js_object_set_field(obj, 2, JsValue::from_number(height as f64));
                            js_object_set_field(obj, 3, JsValue::from_number(channels as f64));
                            js_object_set_field(obj, 4, JsValue::from_number(size as f64));
                        }
                        JsValue::from_object_ptr(obj)
                    });
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
                    // Resolve with a REAL Node `Buffer` of the encoded image
                    // bytes. The Buffer must be allocated on the MAIN thread —
                    // the runtime arena is thread-local, so allocating it here
                    // on the blocking-pool thread would dangle once this thread
                    // idles (#1824). `resolve_with` defers construction to the
                    // resolution pump on the main thread. Previously this
                    // base64-encoded the bytes into a string, which silently
                    // corrupted binary output for the common
                    // `.toBuffer().then(b => res.end(b))` pattern.
                    promise.resolve_with(move || {
                        let buf = alloc_buffer(&bytes);
                        JsValue::from_object_ptr(buf)
                    });
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
            // sharp's metadata resolves a real object, not a string. Do the
            // image inspection here (Send data), build the JS object on the
            // main thread (#1824).
            let (width, height) = sharp.image.dimensions();
            let color = sharp.image.color();
            let channels = color.channel_count();
            let has_alpha = color.has_alpha();
            let space: &str = if color.has_color() { "srgb" } else { "b-w" };
            let format = fmt_name(sharp.format).to_string();
            let space = space.to_string();
            promise.resolve_with(move || {
                let (packed, shape_id) = build_object_shape(&[
                    "format", "width", "height", "channels", "space", "hasAlpha",
                ]);
                let obj = unsafe {
                    js_object_alloc_with_shape(shape_id, 6, packed.as_ptr(), packed.len() as u32)
                };
                unsafe {
                    js_object_set_field(
                        obj,
                        0,
                        JsValue::from_string_ptr(alloc_string(&format).as_raw()),
                    );
                    js_object_set_field(obj, 1, JsValue::from_number(width as f64));
                    js_object_set_field(obj, 2, JsValue::from_number(height as f64));
                    js_object_set_field(obj, 3, JsValue::from_number(channels as f64));
                    js_object_set_field(
                        obj,
                        4,
                        JsValue::from_string_ptr(alloc_string(&space).as_raw()),
                    );
                    js_object_set_field(obj, 5, JsValue::from_bool(has_alpha));
                }
                JsValue::from_object_ptr(obj)
            });
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

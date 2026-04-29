//! Sharp module
//!
//! Native implementation of the 'sharp' npm package using the image crate.
//! Provides image processing functionality.

use crate::common::{get_handle, register_handle, spawn_for_promise, Handle};
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use perry_runtime::{js_promise_new, js_string_from_bytes, JSValue, Promise, StringHeader};
use std::io::Cursor;

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Helper to extract bytes from StringHeader pointer
unsafe fn bytes_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(bytes.to_vec())
}

/// Sharp image handle with pending operations
pub struct SharpHandle {
    pub image: DynamicImage,
    pub format: ImageFormat,
    pub quality: u8,
}

/// sharp(input) -> Sharp
///
/// Create a new Sharp instance from a file path.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_from_file(path_ptr: *const StringHeader) -> Handle {
    let path = match string_from_header(path_ptr) {
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

/// sharp(buffer) -> Sharp
///
/// Create a new Sharp instance from a buffer.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_from_buffer(buffer_ptr: *const StringHeader) -> Handle {
    let buffer = match bytes_from_header(buffer_ptr) {
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

/// sharp.resize(width, height?) -> Sharp
///
/// Resize the image.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_resize(handle: Handle, width: f64, height: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let new_width = width as u32;
        let new_height = if height > 0.0 {
            height as u32
        } else {
            // Calculate height to maintain aspect ratio
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

/// sharp.rotate(angle) -> Sharp
///
/// Rotate the image by the given angle (90, 180, 270).
#[no_mangle]
pub unsafe extern "C" fn js_sharp_rotate(handle: Handle, angle: f64) -> Handle {
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

/// sharp.flip() -> Sharp
///
/// Flip the image vertically.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_flip(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let flipped = sharp.image.flipv();
        return register_handle(SharpHandle {
            image: flipped,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.flop() -> Sharp
///
/// Flip the image horizontally.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_flop(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let flopped = sharp.image.fliph();
        return register_handle(SharpHandle {
            image: flopped,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.grayscale() -> Sharp
///
/// Convert to grayscale.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_grayscale(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let gray = sharp.image.grayscale();
        return register_handle(SharpHandle {
            image: gray,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.blur(sigma) -> Sharp
///
/// Apply Gaussian blur.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_blur(handle: Handle, sigma: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let blurred = sharp.image.blur(sigma as f32);
        return register_handle(SharpHandle {
            image: blurred,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.sharpen() -> Sharp
///
/// Apply sharpening filter.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_sharpen(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let sharpened = sharp.image.unsharpen(1.0, 1);
        return register_handle(SharpHandle {
            image: sharpened,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.crop(left, top, width, height) -> Sharp
///
/// Extract a region from the image.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_crop(
    handle: Handle,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        let cropped = sharp
            .image
            .crop_imm(left as u32, top as u32, width as u32, height as u32);
        return register_handle(SharpHandle {
            image: cropped,
            format: sharp.format,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.jpeg(options?) -> Sharp
///
/// Set output format to JPEG.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_jpeg(handle: Handle, quality: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::Jpeg,
            quality: if quality > 0.0 { quality as u8 } else { 80 },
        });
    }
    -1
}

/// sharp.png() -> Sharp
///
/// Set output format to PNG.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_png(handle: Handle) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::Png,
            quality: sharp.quality,
        });
    }
    -1
}

/// sharp.webp(options?) -> Sharp
///
/// Set output format to WebP.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_webp(handle: Handle, quality: f64) -> Handle {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        return register_handle(SharpHandle {
            image: sharp.image.clone(),
            format: ImageFormat::WebP,
            quality: if quality > 0.0 { quality as u8 } else { 80 },
        });
    }
    -1
}

/// sharp.toFile(path) -> Promise<OutputInfo>
///
/// Write the image to a file.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_to_file(
    handle: Handle,
    path_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let path = match string_from_header(path_ptr) {
        Some(p) => p,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid path".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            match sharp.image.save(&path) {
                Ok(_) => {
                    let (width, height) = sharp.image.dimensions();
                    // Return info as JSON string
                    let info = format!(
                        r#"{{"width":{},"height":{},"format":"{}"}}"#,
                        width,
                        height,
                        match sharp.format {
                            ImageFormat::Jpeg => "jpeg",
                            ImageFormat::Png => "png",
                            ImageFormat::WebP => "webp",
                            ImageFormat::Gif => "gif",
                            _ => "unknown",
                        }
                    );
                    let ptr = js_string_from_bytes(info.as_ptr(), info.len() as u32);
                    Ok(JSValue::string_ptr(ptr).bits())
                }
                Err(e) => Err(format!("Failed to save image: {}", e)),
            }
        } else {
            Err("Invalid sharp handle".to_string())
        }
    });

    promise
}

/// sharp.toBuffer() -> Promise<Buffer>
///
/// Get the image as a buffer.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_to_buffer(handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            let mut buffer = Cursor::new(Vec::new());
            match sharp.image.write_to(&mut buffer, sharp.format) {
                Ok(_) => {
                    let bytes = buffer.into_inner();
                    // Return as hex string for now (or base64)
                    let encoded =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                    let ptr = js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32);
                    Ok(JSValue::string_ptr(ptr).bits())
                }
                Err(e) => Err(format!("Failed to encode image: {}", e)),
            }
        } else {
            Err("Invalid sharp handle".to_string())
        }
    });

    promise
}

/// sharp.metadata() -> Promise<Metadata>
///
/// Get image metadata.
#[no_mangle]
pub unsafe extern "C" fn js_sharp_metadata(handle: Handle) -> *mut Promise {
    let promise = js_promise_new();

    spawn_for_promise(promise as *mut u8, async move {
        if let Some(sharp) = get_handle::<SharpHandle>(handle) {
            let (width, height) = sharp.image.dimensions();
            let channels = sharp.image.color().channel_count();

            let info = format!(
                r#"{{"width":{},"height":{},"channels":{},"format":"{}"}}"#,
                width,
                height,
                channels,
                match sharp.format {
                    ImageFormat::Jpeg => "jpeg",
                    ImageFormat::Png => "png",
                    ImageFormat::WebP => "webp",
                    ImageFormat::Gif => "gif",
                    _ => "unknown",
                }
            );
            let ptr = js_string_from_bytes(info.as_ptr(), info.len() as u32);
            Ok(JSValue::string_ptr(ptr).bits())
        } else {
            Err("Invalid sharp handle".to_string())
        }
    });

    promise
}

/// Get image width
#[no_mangle]
pub unsafe extern "C" fn js_sharp_width(handle: Handle) -> f64 {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        sharp.image.width() as f64
    } else {
        0.0
    }
}

/// Get image height
#[no_mangle]
pub unsafe extern "C" fn js_sharp_height(handle: Handle) -> f64 {
    if let Some(sharp) = get_handle::<SharpHandle>(handle) {
        sharp.image.height() as f64
    } else {
        0.0
    }
}

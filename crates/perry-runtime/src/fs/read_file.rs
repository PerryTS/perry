//! Source selection is shared by text and Buffer reads. Metadata is a bounded
//! capacity hint; the reader, never a cached file size, determines the result.
use super::*;
use std::io;

fn with_source<T>(
    path_value: f64,
    options_value: f64,
    read: impl FnOnce(&mut fs::File, u64) -> io::Result<T>,
    embedded: impl FnOnce(&[u8]) -> io::Result<T>,
) -> Result<T, FsReadFailure> {
    unsafe {
        if let Some(fd) = numeric_fd_value(path_value) {
            return FD_REGISTRY.with(|registry| {
                let mut registry = registry.borrow_mut();
                let file = registry
                    .get_mut(&fd)
                    .ok_or_else(|| FsReadFailure::read(ebadf_os_error()))?;
                let hint = file.metadata().ok().filter(|m| m.is_file()).map_or(0, |m| {
                    file.stream_position()
                        .map_or(0, |at| m.len().saturating_sub(at))
                });
                read(file, hint).map_err(FsReadFailure::read)
            });
        }
        let Some(path) = decode_path_value(path_value) else {
            return Err(FsReadFailure::open(enoent_os_error(), ""));
        };
        if let Some(bytes) = crate::embedded::lookup(&path) {
            return embedded(bytes).map_err(|error| FsReadFailure::read_at(error, &path));
        }
        if crate::embedded::is_virtual_path(&path) {
            return Err(FsReadFailure::open(enoent_os_error(), &path));
        }
        let mut file = open_file_for_read_flag(&path, &read_file_flag(options_value))
            .map_err(|error| FsReadFailure::open(error, &path))?;
        let hint = file
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .map_or(0, |m| m.len());
        read(&mut file, hint).map_err(|error| FsReadFailure::read_at(error, &path))
    }
}

pub(super) fn read_file_bytes_with_options(
    path: f64,
    options: f64,
) -> Result<Vec<u8>, FsReadFailure> {
    with_source(
        path,
        options,
        |reader, hint| {
            let mut bytes =
                Vec::with_capacity(hint.min(crate::string::MAX_STRING_LENGTH as u64) as usize);
            reader.read_to_end(&mut bytes)?;
            Ok(bytes)
        },
        |bytes| Ok(bytes.to_vec()),
    )
}

pub(super) fn read_file_string_with_options(
    path: f64,
    options: f64,
    encoding: &str,
) -> Result<*mut StringHeader, FsReadFailure> {
    if encoding.eq_ignore_ascii_case("utf8") || encoding.eq_ignore_ascii_case("utf-8") {
        return with_source(path, options, crate::string::read_utf8, |mut bytes| {
            let hint = bytes.len() as u64;
            crate::string::read_utf8(&mut bytes, hint)
        });
    }
    // Node accepts this common fs option but Buffer decoding rejects it,
    // except for an empty read (whose toString returns before decoding).
    if encoding == "buffer" {
        let bytes = read_file_bytes_with_options(path, options)?;
        if bytes.is_empty() {
            return Ok(js_string_from_bytes(std::ptr::null(), 0));
        }
        validate::throw_type_error_with_code("Unknown encoding: buffer", "ERR_UNKNOWN_ENCODING");
    }
    let lower = encoding.to_ascii_lowercase();
    if !matches!(
        lower.as_str(),
        "hex"
            | "base64"
            | "base64url"
            | "latin1"
            | "binary"
            | "ascii"
            | "utf16le"
            | "utf-16le"
            | "ucs2"
            | "ucs-2"
    ) {
        validate::throw_type_error_with_code(
            &format!("The argument 'encoding' is invalid encoding. Received '{encoding}'"),
            "ERR_INVALID_ARG_VALUE",
        );
    }
    let bytes = read_file_bytes_with_options(path, options)?;
    Ok(decode_bytes(&bytes, &lower))
}

fn decode_bytes(bytes: &[u8], encoding: &str) -> *mut StringHeader {
    match encoding {
        "hex" => crate::buffer::hex_encode_into_string(bytes),
        "base64" => crate::buffer::base64_encode_into_string(bytes),
        "base64url" => crate::buffer::base64url_encode_into_string(bytes),
        "latin1" | "binary" | "ascii" => {
            let mut text = String::new();
            for &byte in bytes {
                text.push(char::from(if encoding == "ascii" {
                    byte & 0x7f
                } else {
                    byte
                }));
            }
            js_string_from_bytes(text.as_ptr(), text.len() as u32)
        }
        _ => {
            // UTF-16LE drops an odd byte but preserves lone surrogate units.
            let units = bytes
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]));
            let mut out = Vec::new();
            for point in char::decode_utf16(units) {
                match point {
                    Ok(c) => {
                        let mut buf = [0; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                    Err(e) => {
                        let u = e.unpaired_surrogate();
                        out.extend_from_slice(&[
                            0xe0 | (u >> 12) as u8,
                            0x80 | ((u >> 6) & 63) as u8,
                            0x80 | (u & 63) as u8,
                        ]);
                    }
                }
            }
            crate::string::js_string_from_wtf8_bytes(out.as_ptr(), out.len() as u32)
        }
    }
}

#[cfg(test)]
#[path = "read_file_tests.rs"]
mod tests;

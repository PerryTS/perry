//! The native-preview sync RPC protocol: [kind, bin(method), bin(payload)].
use std::fmt;
use std::io::{Read, Write};

#[derive(Debug)]
pub(super) enum ApiError {
    UnsupportedVersion(String),
    Io(std::io::Error),
    Protocol(String),
    Remote { method: String, message: String },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(found) => write!(f,
                "unsupported tsgo API version {found:?}; expected {}. Continuing without type checking. Install @typescript/native-preview@{}",
                super::SUPPORTED_TSGO_VERSION, super::SUPPORTED_TSGO_VERSION),
            Self::Io(e) => write!(f, "tsgo IPC I/O failed: {e}"),
            Self::Protocol(e) => write!(f, "incompatible tsgo API response: {e}"),
            Self::Remote {method, message} => write!(f, "tsgo {method} failed: {message}"),
        }
    }
}
impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

// A corrupt peer must not request unbounded allocation. Source ASTs and
// project-wide diagnostics can exceed bin16; accept bin32 up to 64 MiB.
const MAX_FIELD: usize = 64 * 1024 * 1024;

fn write_bin(out: &mut impl Write, bytes: &[u8]) -> Result<(), ApiError> {
    let len = bytes.len();
    if len > MAX_FIELD {
        return Err(ApiError::Protocol("field too large".into()));
    }
    if len <= 255 {
        out.write_all(&[0xc4, len as u8])?;
    } else if len <= 65535 {
        out.write_all(&[0xc5])?;
        out.write_all(&(len as u16).to_be_bytes())?;
    } else {
        out.write_all(&[0xc6])?;
        out.write_all(&(len as u32).to_be_bytes())?;
    }
    out.write_all(bytes)?;
    Ok(())
}

pub(super) fn write_request(
    out: &mut impl Write,
    method: &str,
    payload: &[u8],
) -> Result<(), ApiError> {
    out.write_all(&[0x93, 1])?;
    write_bin(out, method.as_bytes())?;
    write_bin(out, payload)?;
    out.flush()?;
    Ok(())
}

fn byte(input: &mut impl Read) -> Result<u8, ApiError> {
    let mut b = [0];
    input.read_exact(&mut b)?;
    Ok(b[0])
}
fn read_bin(input: &mut impl Read) -> Result<Vec<u8>, ApiError> {
    let len = match byte(input)? {
        0xc4 => byte(input)? as usize,
        0xc5 => {
            let mut b = [0; 2];
            input.read_exact(&mut b)?;
            u16::from_be_bytes(b) as usize
        }
        0xc6 => {
            let mut b = [0; 4];
            input.read_exact(&mut b)?;
            u32::from_be_bytes(b) as usize
        }
        marker => {
            return Err(ApiError::Protocol(format!(
                "expected bin field, received 0x{marker:02x}"
            )))
        }
    };
    if len > MAX_FIELD {
        return Err(ApiError::Protocol("field too large".into()));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn read_response(input: &mut impl Read, method: &str) -> Result<Vec<u8>, ApiError> {
    let marker = byte(input)?;
    if marker != 0x93 {
        return Err(ApiError::Protocol(format!(
            "expected tuple marker 0x93, received 0x{marker:02x}"
        )));
    }
    let kind = match byte(input)? {
        0xcc => byte(input)?,
        value => value,
    };
    let name = read_bin(input)?;
    let payload = read_bin(input)?;
    if name != method.as_bytes() {
        return Err(ApiError::Protocol(format!(
            "expected method {method}, received {:?}",
            String::from_utf8_lossy(&name)
        )));
    }
    match kind {
        4 => Ok(payload),
        5 => Err(ApiError::Remote {
            method: method.into(),
            message: String::from_utf8_lossy(&payload).into_owned(),
        }),
        // No filesystem callbacks are enabled for this client.
        other => Err(ApiError::Protocol(format!(
            "unexpected message kind {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_uses_fixed_tuple_and_binary_fields() {
        let mut out = Vec::new();
        write_request(&mut out, "echo", b"{}").unwrap();
        assert_eq!(out, b"\x93\x01\xc4\x04echo\xc4\x02{}");
    }
    #[test]
    fn response_handles_all_binary_lengths_and_concatenated_frames() {
        let mut frames = Vec::new();
        for len in [0, 255, 256, 65535, 65536] {
            frames.extend_from_slice(&[0x93, 0xcc, 4]);
            write_bin(&mut frames, b"echo").unwrap();
            write_bin(&mut frames, &vec![b'x'; len]).unwrap();
        }
        let mut input = Cursor::new(frames);
        for len in [0, 255, 256, 65535, 65536] {
            assert_eq!(read_response(&mut input, "echo").unwrap(), vec![b'x'; len]);
        }
    }
    #[test]
    fn errors_are_distinct_and_malformed_frames_are_rejected() {
        let mut out = vec![0x93, 5];
        write_bin(&mut out, b"echo").unwrap();
        write_bin(&mut out, b"bad request").unwrap();
        assert!(matches!(
            read_response(&mut out.as_slice(), "echo"),
            Err(ApiError::Remote { .. })
        ));
        out[1] = 4;
        assert!(matches!(
            read_response(&mut out.as_slice(), "other"),
            Err(ApiError::Protocol(_))
        ));
        for bytes in [
            &b"\0"[..],
            &b"\x93\x04\xa0"[..],
            &b"\x93\x04\xc6\xff\xff\xff\xff"[..],
        ] {
            assert!(matches!(
                read_response(&mut &*bytes, "echo"),
                Err(ApiError::Protocol(_))
            ));
        }
        assert!(matches!(
            read_response(&mut &b"\x93\x04\xc4\x04ec"[..], "echo"),
            Err(ApiError::Io(_))
        ));
    }
}

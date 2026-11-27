//! `path.join(a, b, ...)` over all of its arguments in one pass.
//!
//! Node joins every non-empty argument with the separator and normalizes the
//! result once (`posix.join`, `win32.join` in `lib/path.js`). The compiled
//! call (`Expr::PathJoin` / `Expr::PathWin32Join`) and the dynamic dispatch
//! (`(0, path.join)(...)` through a value) both land here with the NaN-boxed
//! arguments as one slice.
//!
//! The posix join reads each argument's bytes in place (heap payload or the
//! inline short-string bytes) and normalizes them straight into one output
//! buffer, segment by segment, exactly as Node's `normalizeString` does over
//! the joined string, so it builds neither the joined string nor a segment
//! list. Nothing is allocated on the GC heap until the result string, so no
//! argument has to be rooted while the others are read. The output is never
//! longer than the joined input plus two bytes (`./`), which bounds the
//! buffer: on the stack for ordinary paths, one `Vec` for long ones.
//!
//! Bytes, not `str`: a separator or a dot is one ASCII byte and never occurs
//! inside a multi-byte sequence, so a WTF-8 payload (a lone surrogate) joins
//! like any other string.

use crate::string::{js_string_from_bytes, with_string_value_bytes, StringHeader};

/// Output buffers up to this size live on the stack.
const STACK_OUT: usize = 512;

/// Node's `validateString(arg, "path")`: a non-string argument throws
/// `ERR_INVALID_ARG_TYPE`, in argument order.
fn arg_len(value: f64) -> usize {
    with_string_value_bytes(value, |b| b.len())
        .unwrap_or_else(|| super::throw_invalid_path_arg_type())
}

/// Node's `normalizeString` for `/`, fed one segment at a time. `out[..len]`
/// is `res`; `last_seg` is `lastSegmentLength`.
struct Normalizer<'o> {
    out: &'o mut [u8],
    start: usize,
    len: usize,
    last_seg: usize,
    allow_above_root: bool,
}

impl Normalizer<'_> {
    fn res(&self) -> &[u8] {
        &self.out[self.start..self.len]
    }

    fn push(&mut self, seg: &[u8]) {
        if self.len > self.start {
            self.out[self.len] = b'/';
            self.len += 1;
        }
        self.out[self.len..self.len + seg.len()].copy_from_slice(seg);
        self.len += seg.len();
        self.last_seg = seg.len();
    }

    fn segment(&mut self, seg: &[u8]) {
        match seg {
            b"" | b"." => {}
            b".." => {
                let res = self.res();
                let last_is_dotdot = res.len() >= 2 && self.last_seg == 2 && res.ends_with(b"..");
                if !last_is_dotdot && !res.is_empty() {
                    // Drop the last segment (`res` of two bytes or less is one
                    // segment).
                    let cut = if res.len() > 2 {
                        res.iter().rposition(|&c| c == b'/').unwrap_or(0)
                    } else {
                        0
                    };
                    self.len = self.start + cut;
                    let res = self.res();
                    self.last_seg =
                        res.len() - res.iter().rposition(|&c| c == b'/').map_or(0, |p| p + 1);
                    return;
                }
                if self.allow_above_root {
                    self.push(b"..");
                }
            }
            _ => self.push(seg),
        }
    }
}

/// `posix.join(...args)` into `out` (at least the joined length plus two
/// bytes); returns the result length.
fn posix_join_into(args: &[f64], out: &mut [u8]) -> usize {
    let first = args.iter().position(|&v| arg_len(v) > 0);
    let Some(first) = first else {
        out[0] = b'.';
        return 1;
    };
    let last = args.iter().rposition(|&v| arg_len(v) > 0).unwrap_or(first);
    let is_absolute = with_string_value_bytes(args[first], |b| b[0] == b'/').unwrap_or(false);
    let trailing =
        with_string_value_bytes(args[last], |b| b[b.len() - 1] == b'/').unwrap_or(false);
    let start = usize::from(is_absolute);
    if is_absolute {
        out[0] = b'/';
    }
    let mut norm = Normalizer {
        out,
        start,
        len: start,
        last_seg: 0,
        allow_above_root: !is_absolute,
    };
    for &v in &args[first..=last] {
        with_string_value_bytes(v, |bytes| {
            for seg in bytes.split(|&c| c == b'/') {
                norm.segment(seg);
            }
        });
    }
    let mut len = norm.len;
    let out = norm.out;
    if len == start {
        if is_absolute {
            return 1;
        }
        let dot: &[u8] = if trailing { b"./" } else { b"." };
        out[..dot.len()].copy_from_slice(dot);
        return dot.len();
    }
    if trailing {
        out[len] = b'/';
        len += 1;
    }
    len
}

/// Run `f` over an output buffer of `cap` bytes and make the JS string of the
/// prefix it reports.
fn with_out(cap: usize, f: impl FnOnce(&mut [u8]) -> usize) -> *mut StringHeader {
    if cap <= STACK_OUT {
        let mut buf = [0u8; STACK_OUT];
        let n = f(&mut buf[..cap]);
        js_string_from_bytes(buf.as_ptr(), n as u32)
    } else {
        let mut buf = vec![0u8; cap];
        let n = f(&mut buf);
        js_string_from_bytes(buf.as_ptr(), n as u32)
    }
}

/// Every argument validated, then the joined length (with separators).
fn joined_len(args: &[f64]) -> usize {
    args.iter().map(|&v| arg_len(v) + 1).sum()
}

pub(crate) fn posix_join_values(args: &[f64]) -> *mut StringHeader {
    let cap = joined_len(args) + 2;
    with_out(cap, |out| posix_join_into(args, out))
}

/// Node's Windows reserved device names (`isWindowsReservedName`).
fn is_windows_reserved_name(part: &[u8], colon: usize) -> bool {
    let device = &part[..colon];
    let eq = |name: &[u8]| device.eq_ignore_ascii_case(name);
    if ["CON", "PRN", "AUX", "NUL"].iter().any(|n| eq(n.as_bytes())) {
        return true;
    }
    // COM1-9 / LPT1-9, and the superscript digits ¹ ² ³ (UTF-8 C2 B9/B2/B3).
    if device.len() < 4 || !(device[..3].eq_ignore_ascii_case(b"COM") || device[..3].eq_ignore_ascii_case(b"LPT")) {
        return false;
    }
    matches!(&device[3..], [b'1'..=b'9'] | [0xC2, 0xB9] | [0xC2, 0xB2] | [0xC2, 0xB3])
}

fn is_win32_sep(c: u8) -> bool {
    c == b'/' || c == b'\\'
}

/// `win32.join(...args)`: Node's join, then the win32 normalizer.
pub(crate) fn win32_join_values(args: &[f64]) -> *mut StringHeader {
    let cap = joined_len(args);
    let mut joined: Vec<u8> = Vec::with_capacity(cap);
    let mut first_part: Option<(bool, bool, Option<bool>)> = None;
    for &v in args {
        with_string_value_bytes(v, |b| {
            if b.is_empty() {
                return;
            }
            if first_part.is_none() {
                first_part = Some((
                    is_win32_sep(b[0]),
                    b.len() > 1 && is_win32_sep(b[1]),
                    (b.len() > 2).then(|| is_win32_sep(b[2])),
                ));
            } else {
                joined.push(b'\\');
            }
            joined.extend_from_slice(b);
        });
    }
    let Some((s0, s1, s2)) = first_part else {
        return js_string_from_bytes(b".".as_ptr(), 1);
    };
    // Keep a UNC root (exactly two separators, then a name) in the first
    // part; otherwise collapse a run of leading separators to one.
    let unc = s0 && s1 && s2 == Some(false);
    if !unc {
        let slashes = joined.iter().take_while(|&&c| is_win32_sep(c)).count();
        if slashes >= 2 {
            joined.drain(..slashes - 1);
            joined[0] = b'\\';
        }
    }
    let reserved = joined
        .split(|&c| c == b'\\')
        .filter(|p| !p.is_empty())
        .any(|p| p.iter().position(|&c| c == b':').is_some_and(|colon| is_windows_reserved_name(p, colon)));
    if reserved {
        for c in joined.iter_mut() {
            if *c == b'/' {
                *c = b'\\';
            }
        }
        return js_string_from_bytes(joined.as_ptr(), joined.len() as u32);
    }
    let Ok(joined) = std::str::from_utf8(&joined) else {
        super::throw_invalid_path_arg_type()
    };
    super::string_to_js(&super::normalize_win32_str(joined))
}

/// `path.join(...)` with the platform's semantics (win32 on Windows targets).
pub(crate) fn join_values(args: &[f64]) -> *mut StringHeader {
    if cfg!(windows) {
        win32_join_values(args)
    } else {
        posix_join_values(args)
    }
}

/// # Safety
/// `args` points to `n` NaN-boxed values.
unsafe fn args_slice<'a>(args: *const f64, n: u32) -> &'a [f64] {
    if n == 0 || args.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args, n as usize)
    }
}

/// Compiled `path.join(a, b, ...)`: `n` NaN-boxed arguments at `args`.
#[no_mangle]
pub unsafe extern "C" fn js_path_join_values(args: *const f64, n: u32) -> *mut StringHeader {
    join_values(args_slice(args, n))
}

/// Compiled `path.win32.join(a, b, ...)`.
#[no_mangle]
pub unsafe extern "C" fn js_path_win32_join_values(args: *const f64, n: u32) -> *mut StringHeader {
    win32_join_values(args_slice(args, n))
}

/// Keepalive anchors: emitted only from generated code.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PATH_JOIN_VALUES: unsafe extern "C" fn(*const f64, u32) -> *mut StringHeader =
    js_path_join_values;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PATH_WIN32_JOIN_VALUES: unsafe extern "C" fn(*const f64, u32) -> *mut StringHeader =
    js_path_win32_join_values;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::JSValue;

    fn value(s: &str) -> f64 {
        // Short strings take the inline form, longer ones the heap form.
        match JSValue::try_short_string(s.as_bytes()) {
            Some(v) if v.is_short_string() => f64::from_bits(v.bits()),
            _ => {
                let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                f64::from_bits(JSValue::string_ptr(ptr).bits())
            }
        }
    }

    fn read(ptr: *mut StringHeader) -> String {
        unsafe {
            let len = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            String::from_utf8(std::slice::from_raw_parts(data, len).to_vec()).unwrap()
        }
    }

    fn join(win32: bool, parts: &[&str]) -> String {
        let args: Vec<f64> = parts.iter().map(|s| value(s)).collect();
        read(if win32 { win32_join_values(&args) } else { posix_join_values(&args) })
    }

    /// Expected values are node's `path.posix.join` (v26).
    #[test]
    fn posix_join_matches_node() {
        let _suppress = crate::gc::GcSuppressScope::new();
        for (parts, expected) in [
            (&[][..], "."),
            (&[""], "."),
            (&["", ""], "."),
            (&["."], "."),
            (&[".", "."], "."),
            (&["./"], "./"),
            (&["/"], "/"),
            (&["//"], "/"),
            (&["", "/"], "/"),
            (&["/", ""], "/"),
            (&["a", ""], "a"),
            (&["", "a"], "a"),
            (&["a", "b"], "a/b"),
            (&["/a", "b"], "/a/b"),
            (&["a/", "b/"], "a/b/"),
            (&["/a/", "/b/"], "/a/b/"),
            (&["a//b", "c"], "a/b/c"),
            (&["a", "..", "b"], "b"),
            (&["..", "a"], "../a"),
            (&["/..", "a"], "/a"),
            (&["/a", "..", ".."], "/"),
            (&["/a", "../../.."], "/"),
            (&["a", "../../b"], "../b"),
            (&["../..", ".."], "../../.."),
            (&["a/b", "../../.."], ".."),
            (&["./a", "./b/."], "a/b"),
            (&["a", "b", "./"], "a/b/"),
            (&["a", "b", ".."], "a"),
            (&["a", "b", "../"], "a/"),
            (&["...", "a"], ".../a"),
            (&["a..", ".."], "."),
            (&[".a", "..b", "..."], ".a/..b/..."),
            (&["/x/y", "", "z"], "/x/y/z"),
            (&["a/./b/.", "c/./"], "a/b/c/"),
            (&["//a", "b"], "/a/b"),
            (&["ü", "ß", "..", "日本"], "ü/日本"),
            (&["/srv/app", ".next", "routes-manifest.json"], "/srv/app/.next/routes-manifest.json"),
        ] {
            assert_eq!(join(false, parts), expected, "posix.join{parts:?}");
        }
        let long = join(false, &[&"a".repeat(600), &"b".repeat(600), ".."]);
        assert_eq!(long, "a".repeat(600), "spilled output buffer");
    }

    /// Expected values are node's `path.win32.join` (v26).
    #[test]
    fn win32_join_matches_node() {
        let _suppress = crate::gc::GcSuppressScope::new();
        for (parts, expected) in [
            (&[][..], "."),
            (&[""], "."),
            (&["a", "b"], r"a\b"),
            (&[r"C:\a", r"..\b"], r"C:\b"),
            (&["C:", "a"], r"C:\a"),
            (&[r"\\server", "share"], r"\\server\share\"),
            (&["//server", "share", "x"], r"\\server\share\x"),
            (&[r"\\\server", "share"], r"\server\share"),
            (&["/", "/server"], r"\server"),
            (&["\\", r"\server\share"], r"\server\share"),
            (&["a/", "b\\"], r"a\b\"),
            (&["CON:", "x"], r"CON:\x"),
            (&["a", "com1:b"], r"a\com1:b"),
            (&["a", "..", ".."], ".."),
            (&["./", "x"], "x"),
        ] {
            assert_eq!(join(true, parts), expected, "win32.join{parts:?}");
        }
    }
}

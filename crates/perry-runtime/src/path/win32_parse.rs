/// Lexical Windows components, retaining every separator from the input.
/// Boundaries are ASCII separators/colons/dots, so slicing also preserves UTF-8.
pub(super) fn parse_components(path: &str) -> [&str; 5] {
    let bytes = path.as_bytes();
    let len = bytes.len();
    let sep = |b: u8| matches!(b, b'/' | b'\\');
    let mut root_end = 0;
    if len > 0 && sep(bytes[0]) {
        root_end = 1;
        if len > 1 && sep(bytes[1]) {
            // A UNC root needs both a nonempty server and a nonempty share.
            let mut cursor = 2;
            while cursor < len && !sep(bytes[cursor]) {
                cursor += 1;
            }
            if cursor > 2 && cursor < len {
                while cursor < len && sep(bytes[cursor]) {
                    cursor += 1;
                }
                let share_start = cursor;
                while cursor < len && !sep(bytes[cursor]) {
                    cursor += 1;
                }
                if cursor > share_start {
                    root_end = if cursor < len { cursor + 1 } else { cursor };
                }
            }
        }
    } else if len >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        root_end = if len > 2 && sep(bytes[2]) { 3 } else { 2 };
    }

    let mut end = len;
    while end > root_end && sep(bytes[end - 1]) {
        end -= 1;
    }
    let mut start = end;
    while start > root_end && !sep(bytes[start - 1]) {
        start -= 1;
    }
    let root = &path[..root_end];
    let dir = if start > root_end {
        &path[..start - 1]
    } else {
        root
    };
    let base = &path[start..end];
    let (ext, name) = match base.rfind('.') {
        Some(index) if index > 0 && base != ".." => (&base[index..], &base[..index]),
        _ => ("", base),
    };
    [root, dir, base, ext, name]
}

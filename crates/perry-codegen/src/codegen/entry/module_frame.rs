//! Static stack identity of source-executing module bodies.

use super::*;

pub(super) fn record(llmod: &mut LlModule, hir: &HirModule, body: &str, source: Option<&str>) {
    // CommonJS source runs inside its already named factory. Its initializer
    // only calls that factory and must not add a second source frame.
    let name = if crate::collectors::is_cjs_wrapped_module(hir) {
        None
    } else {
        let url = file_url(&perry_hir::source_remap::baked_path(
            source.unwrap_or(&hir.name),
        ));
        let (global, byte_len) = llmod.add_string_constant(&url);
        Some(crate::fn_info::RetainedName {
            global,
            byte_len,
            image: false,
        })
    };
    llmod.note_fn_info(body, |facts| {
        facts.module_frame = true;
        facts.name = name;
    });
}

fn file_url(path: &str) -> String {
    use std::fmt::Write;
    let path = path.replace('\\', "/");
    let path = if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_string()
    };
    let mut url = if path.starts_with("//") {
        "file:"
    } else if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    }
    .to_string();
    for byte in path.bytes() {
        if byte <= b' ' || byte >= 0x7f || b"\"#%<>?`{}".contains(&byte) {
            write!(url, "%{byte:02X}").unwrap();
        } else {
            url.push(char::from(byte));
        }
    }
    url
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_paths_keep_file_url_syntax() {
        assert_eq!(
            super::file_url("/app/a b#c%.mjs"),
            "file:///app/a%20b%23c%25.mjs"
        );
        assert_eq!(super::file_url(r"\\?\C:\app\a.mjs"), "file:///C:/app/a.mjs");
        assert_eq!(
            super::file_url(r"\\?\UNC\server\share\a.mjs"),
            "file://server/share/a.mjs"
        );
    }
}

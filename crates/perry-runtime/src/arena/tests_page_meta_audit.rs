//! Source inventory shared by the page-index obligation tests.
//! Discover every Rust module so a later split cannot silently narrow coverage.

pub(super) fn source() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/arena/page_meta");
    let mut pending = vec![root];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("page-meta directory readable") {
            let path = entry.expect("page-meta entry readable").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut source = String::new();
    for path in files {
        source.push_str(&std::fs::read_to_string(&path).expect("page-meta source readable"));
        source.push('\n');
    }
    source
}

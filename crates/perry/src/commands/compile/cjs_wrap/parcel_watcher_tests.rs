use super::*;
use std::path::PathBuf;

#[test]
fn wrap_folds_opencode_parcel_watcher_template_require_for_target() {
    let src = r#"
const libc = typeof OPENCODE_LIBC === "undefined" ? undefined : OPENCODE_LIBC
const binding = require(
  `@parcel/watcher-${process.platform}-${process.arch}${process.platform === "linux" ? `-${libc || "glibc"}` : ""}`,
)
module.exports = binding
"#;
    let wrapped = wrap_commonjs_for_target(
        src,
        &PathBuf::from("/tmp/node_modules/opencode/watcher.js"),
        Some("linux-x86_64-musl"),
    );
    assert!(
        wrapped.contains("from '@parcel/watcher-linux-x64-musl'")
            || wrapped.contains("from \"@parcel/watcher-linux-x64-musl\""),
        "target-specific sidecar must become a static import:\n{wrapped}"
    );
    assert!(!wrapped.contains("require(\n  `@parcel/watcher-"));
}

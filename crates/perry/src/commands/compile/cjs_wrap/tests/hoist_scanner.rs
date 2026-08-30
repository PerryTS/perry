//! `hoist_classes` top-level-binding scanner regression — split out of
//! `tests.rs`, which had crossed the 2000-line size gate.

use super::super::hoist_classes::extract_top_level_class_decls;

#[test]
fn regex_quote_before_local_superclass_keeps_class_in_cjs_iife() {
    // @smithy/core's serde CJS emit contains this sequence. The quote inside
    // the regex is not a string delimiter; treating it as one desynchronized
    // the top-level-binding scanner, hid `ReadableStreamRef`, and hoisted only
    // `ChecksumStream` ahead of the CommonJS IIFE.
    let src = r#"const splitHeader = (value) => {
    return value.replace(/\\"/g, '"');
};
const ReadableStreamRef = typeof ReadableStream === "function"
    ? ReadableStream
    : function () {};
class ChecksumStream extends ReadableStreamRef {}
module.exports = { ChecksumStream };
"#;

    let (blocks, hoisted_names, rest) = extract_top_level_class_decls(src);
    assert!(
        !hoisted_names.iter().any(|name| name == "ChecksumStream"),
        "a class depending on a CJS-local superclass must not hoist; hoisted block:\n{blocks}"
    );
    assert!(
        rest.contains("class ChecksumStream extends ReadableStreamRef"),
        "the class declaration must remain at its source position inside the factory"
    );
}

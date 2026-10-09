//! Names in compiler-packed, NUL-terminated object key lists.

/// The key names in a compiler-packed key list. Codegen writes every name
/// followed by a NUL (`codegen/mod.rs`, `expr/object_literal.rs`,
/// `lower_call/new_alloc.rs`), so the names are the segments between the
/// terminators, and an empty segment is a name: the key `""`. Only the empty
/// segment after the final terminator is dropped.
///
/// Every reader used to drop ALL empty segments, so `{ "": v }` built a keys
/// array shorter than its count and the shape mint refused the facts (an
/// abort at the literal), and two modules' `{ "": v }` -- equal contents under
/// one static ShapeId -- each built their own empty array, so the second
/// module's static mint was refused (OpenCode's TUI: json5's reviver holder
/// `{ "": root }`).
pub(crate) fn packed_key_names(bytes: &[u8]) -> Vec<&[u8]> {
    let mut names: Vec<&[u8]> = bytes.split(|&b| b == 0).collect();
    if names.last().is_some_and(|s| s.is_empty()) {
        names.pop();
    }
    names
}

#[cfg(test)]
mod packed_key_names_tests {
    use super::packed_key_names;

    #[test]
    fn the_empty_key_is_a_name_and_only_the_final_terminator_is_dropped() {
        let names =
            |b: &[u8]| -> Vec<Vec<u8>> { packed_key_names(b).iter().map(|s| s.to_vec()).collect() };
        assert_eq!(names(b"a\0b\0"), vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(
            names(b"\0"),
            vec![b"".to_vec()],
            "{{ \"\": v }} has one key"
        );
        assert_eq!(names(b"\0a\0"), vec![b"".to_vec(), b"a".to_vec()]);
        assert_eq!(names(b"a\0\0"), vec![b"a".to_vec(), b"".to_vec()]);
        assert_eq!(
            names(b"a\0b"),
            vec![b"a".to_vec(), b"b".to_vec()],
            "an unterminated last name"
        );
        assert!(names(b"").is_empty());
    }
}

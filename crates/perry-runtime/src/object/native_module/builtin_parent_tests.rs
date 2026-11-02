use super::*;

/// `repl.Recoverable extends SyntaxError`: the link reads the builtin
/// parent's `prototype` and leaves its descriptor as SyntaxError's own
/// declaration gives it (non-writable, as in Node).
#[test]
fn linking_a_native_subclass_keeps_the_builtin_parent_prototype_descriptor() {
    std::thread::spawn(|| {
        let parent = crate::object::js_get_global_this_builtin_value(b"SyntaxError".as_ptr(), 11);
        let ptr = crate::value::js_nanbox_get_pointer(parent) as usize;
        let writable =
            || crate::object::get_property_attrs(ptr, "prototype").is_some_and(|a| a.writable());
        assert!(!writable());
        let recoverable = unsafe {
            js_native_module_property_by_name(b"repl".as_ptr(), 4, b"Recoverable".as_ptr(), 11)
        };
        assert_eq!(
            crate::object::js_object_get_prototype_of(recoverable).to_bits(),
            parent.to_bits()
        );
        assert!(
            !writable(),
            "linking repl.Recoverable made SyntaxError.prototype writable"
        );
    })
    .join()
    .unwrap();
}

//! Unit tests for the `path` module — split out of `path.rs` to keep that
//! file under the CI 2,000-line cap. Child module of `path`, so the parent's
//! private helpers (`parse_posix_components`, `string_to_js`, the win32
//! inners, …) are directly accessible.

mod posix_parse_tests {
    use super::super::parse_posix_components;

    fn parse(path: &str) -> (String, String, String, String, String) {
        parse_posix_components(path)
    }

    #[test]
    fn final_dot_segments_are_literal_base_names() {
        assert_eq!(
            parse("/tmp/."),
            (
                "/".to_string(),
                "/tmp".to_string(),
                ".".to_string(),
                String::new(),
                ".".to_string()
            )
        );
        assert_eq!(
            parse("/tmp/.."),
            (
                "/".to_string(),
                "/tmp".to_string(),
                "..".to_string(),
                String::new(),
                "..".to_string()
            )
        );
    }

    #[test]
    fn trailing_separators_are_ignored_without_normalizing() {
        assert_eq!(
            parse("/foo//bar//"),
            (
                "/".to_string(),
                "/foo/".to_string(),
                "bar".to_string(),
                String::new(),
                "bar".to_string()
            )
        );
        assert_eq!(
            parse("foo//"),
            (
                String::new(),
                String::new(),
                "foo".to_string(),
                String::new(),
                "foo".to_string()
            )
        );
    }

    #[test]
    fn dotfile_extension_rules_match_node() {
        assert_eq!(
            parse("/.bashrc"),
            (
                "/".to_string(),
                "/".to_string(),
                ".bashrc".to_string(),
                String::new(),
                ".bashrc".to_string()
            )
        );
        assert_eq!(
            parse(".profile.js"),
            (
                String::new(),
                String::new(),
                ".profile.js".to_string(),
                ".js".to_string(),
                ".profile".to_string()
            )
        );
    }
}

#[cfg(feature = "regex-engine")]
mod glob_tests {
    use super::super::glob_to_regex;

    #[test]
    fn brace_alternation_expands_to_group() {
        assert_eq!(glob_to_regex("*.{md,txt}"), "^[^/]*\\.(?:md|txt)$");
        assert_eq!(
            glob_to_regex("src/{app,test}.ts"),
            "^src/(?:app|test)\\.ts$"
        );
    }

    #[test]
    fn braces_without_alternation_stay_literal() {
        assert_eq!(glob_to_regex("file.{md}"), "^file\\.\\{md\\}$");
    }

    #[test]
    fn extglob_positive_groups_expand() {
        assert_eq!(glob_to_regex("*.@(js|ts)"), "^[^/]*\\.(?:js|ts)$");
        assert_eq!(glob_to_regex("*.+(js|ts)"), "^[^/]*\\.(?:js|ts)+$");
        assert_eq!(glob_to_regex("*.?(js|ts)"), "^[^/]*\\.(?:js|ts)?$");
    }

    #[test]
    fn globstar_is_segment_aware() {
        assert_eq!(glob_to_regex("a/**/c"), "^a/(?:[^/]+/)*c$");
        assert_eq!(glob_to_regex("a/**"), "^a/.*$");
        assert_eq!(glob_to_regex("a**b"), "^a[^/]*b$");
    }

    #[test]
    fn pattern_backslashes_are_separators() {
        assert_eq!(glob_to_regex("foo\\*"), "^foo/[^/]*$");
    }
}

mod win32_normalize_tests {
    use super::super::{
        current_dir_as_win32, join_win32_paths, normalize_win32_str, posix_cwd_as_win32_path,
        win32_basename_inner, win32_dirname_inner, win32_resolve_inner, win32_to_namespaced_path,
    };

    #[test]
    fn drive_relative_bare_appends_dot() {
        // #1728: a bare drive ref is the drive's *current dir*, not the root.
        assert_eq!(normalize_win32_str("C:"), "C:.");
        assert_eq!(normalize_win32_str("c:"), "c:.");
    }

    #[test]
    fn trailing_separator_preserved() {
        // #1728: a trailing separator the input carried is kept.
        assert_eq!(normalize_win32_str(".\\"), ".\\");
        assert_eq!(normalize_win32_str("C:\\foo\\"), "C:\\foo\\");
        assert_eq!(normalize_win32_str("\\\\?\\C:\\"), "\\\\?\\C:\\");
    }

    #[test]
    fn unc_root_keeps_trailing_separator() {
        // #1728: a bare UNC/device root normalizes with a trailing separator.
        assert_eq!(
            normalize_win32_str("\\\\server\\share"),
            "\\\\server\\share\\"
        );
        assert_eq!(
            normalize_win32_str("\\\\server\\share\\"),
            "\\\\server\\share\\"
        );
        // Content after the root is unaffected (no spurious trailing sep).
        assert_eq!(
            normalize_win32_str("\\\\server\\share\\foo\\..\\bar"),
            "\\\\server\\share\\bar"
        );
        assert_eq!(
            normalize_win32_str("//server/share/a/b"),
            "\\\\server\\share\\a\\b"
        );
    }

    #[test]
    fn basename_handles_unc_root_and_drive() {
        // #1728: win32.basename of a UNC root is the share segment.
        assert_eq!(win32_basename_inner("\\\\server\\share\\"), "share");
        assert_eq!(win32_basename_inner("\\\\server\\share\\file"), "file");
        assert_eq!(win32_basename_inner("C:\\foo\\bar\\baz.txt"), "baz.txt");
        assert_eq!(win32_basename_inner("C:foo"), "foo");
    }

    #[test]
    fn dirname_preserves_input_separator_style() {
        assert_eq!(win32_dirname_inner("/foo/bar"), "/foo");
        assert_eq!(win32_dirname_inner("/foo/bar/"), "/foo");
        assert_eq!(win32_dirname_inner("foo/bar/baz"), "foo/bar");
        assert_eq!(win32_dirname_inner("C:/foo/bar"), "C:/foo");
        assert_eq!(win32_dirname_inner("//server/share"), "//server/share");
        assert_eq!(win32_dirname_inner("//server/share/a"), "//server/share/");
    }

    #[test]
    fn drive_relative_with_segments_unchanged() {
        // The `.` is only appended when there are no segments.
        assert_eq!(normalize_win32_str("C:foo"), "C:foo");
        assert_eq!(normalize_win32_str("C:.."), "C:..");
        assert_eq!(normalize_win32_str("C:foo\\bar"), "C:foo\\bar");
    }

    #[test]
    fn drive_absolute_and_others_unaffected() {
        // Regression guard for the cases that already matched Node.
        assert_eq!(normalize_win32_str("C:\\"), "C:\\");
        assert_eq!(normalize_win32_str("C:\\foo"), "C:\\foo");
        assert_eq!(normalize_win32_str("a//b//../b"), "a\\b");
        assert_eq!(normalize_win32_str("/foo/../../../bar"), "\\bar");
        assert_eq!(normalize_win32_str(""), ".");
    }

    /// POSIX host: the cwd is driveless, so a drive-relative input grafts
    /// its drive onto the cwd (historical Perry behavior).
    #[cfg(not(windows))]
    #[test]
    fn resolve_drive_relative_uses_posix_cwd_as_drive_cwd() {
        let cwd = posix_cwd_as_win32_path();
        let drive_cwd = format!("C:{}", cwd);
        assert_eq!(
            win32_resolve_inner("C:foo"),
            normalize_win32_str(&join_win32_paths(&drive_cwd, "foo"))
        );
        assert_ne!(win32_resolve_inner("C:foo"), "C:\\C:foo");
        assert_eq!(
            win32_resolve_inner("foo"),
            normalize_win32_str(&join_win32_paths(&cwd, "foo"))
        );
    }

    /// Windows host: the cwd already carries a drive. Same-drive inputs
    /// resolve against the cwd; different-drive inputs fall back to that
    /// drive's root (never the pre-fix `C:C:\...` graft).
    #[cfg(windows)]
    #[test]
    fn resolve_drive_relative_uses_host_cwd_drive() {
        let cwd = posix_cwd_as_win32_path();
        let cwd_drive = &cwd[..2]; // e.g. "C:"
        assert!(cwd_drive.ends_with(':'), "cwd {cwd:?} has no drive prefix");

        let same_drive_input = format!("{}foo", cwd_drive);
        assert_eq!(
            win32_resolve_inner(&same_drive_input),
            normalize_win32_str(&join_win32_paths(&cwd, "foo"))
        );
        assert!(!win32_resolve_inner(&same_drive_input).contains(":\\C:"));

        // A drive that is NOT the cwd's drive resolves from its root.
        let other = if cwd_drive.eq_ignore_ascii_case("q:") {
            "z:"
        } else {
            "q:"
        };
        assert_eq!(
            win32_resolve_inner(&format!("{}foo", other)),
            format!("{}\\foo", other)
        );

        assert_eq!(
            win32_resolve_inner("foo"),
            normalize_win32_str(&join_win32_paths(&cwd, "foo"))
        );
    }

    #[test]
    fn to_namespaced_path_resolves_but_only_namespaces_drive_and_unc() {
        let cwd = current_dir_as_win32().unwrap();
        let resolved_relative = normalize_win32_str(&format!("{}\\foo", cwd));
        // On a Windows host the cwd is drive-absolute, so the resolved
        // relative path gets the `\\?\` device prefix (matching Node); on a
        // POSIX host the cwd is driveless and no prefix applies.
        let expected_relative = if cfg!(windows) {
            format!("\\\\?\\{}", resolved_relative)
        } else {
            resolved_relative
        };
        assert_eq!(win32_to_namespaced_path("foo"), expected_relative);
        assert_eq!(win32_to_namespaced_path("/tmp/x"), "\\tmp\\x");
        assert_eq!(win32_to_namespaced_path("C:\\foo"), "\\\\?\\C:\\foo");
        assert_eq!(
            win32_to_namespaced_path("\\\\server\\share\\file"),
            "\\\\?\\UNC\\server\\share\\file"
        );
        assert_eq!(
            win32_to_namespaced_path("\\\\?\\C:\\already"),
            "\\\\?\\C:\\already"
        );
    }
}

mod malloc_backed_string_arg_tests {
    use super::super::*;

    /// Build a REAL string whose backing allocation is malloc-tracked (not
    /// arena) — the shape produced by the string-append realloc path
    /// (`gc_malloc_realloc` → `gc_malloc(_, GC_TYPE_STRING)`) and by Symbol
    /// descriptions. Its user address classifies as `HeapGeneration::Unknown`.
    fn malloc_backed_string(bytes: &[u8]) -> *mut StringHeader {
        let payload = std::mem::size_of::<StringHeader>() + bytes.len();
        let user = crate::gc::gc_malloc(payload, crate::gc::GC_TYPE_STRING) as *mut StringHeader;
        assert!(!user.is_null());
        unsafe {
            crate::string::init_string_header(
                user,
                bytes.len() as u32,
                bytes.len() as u32,
                bytes.len() as u32,
                0,
                0,
            );
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (user as *mut u8).add(std::mem::size_of::<StringHeader>()),
                bytes.len(),
            );
        }
        user
    }

    /// A malloc-backed (generation-Unknown) string is a valid `path` argument.
    /// `is_string_header_ptr` used to reject every generation-Unknown pointer,
    /// so `path.dirname()` (and every other `path.*` builtin) threw
    /// `TypeError: The "path" argument must be of type string.` whenever a
    /// program passed a string that had grown through the append realloc path.
    /// (The `/`-separated input yields the same dirname/basename under both
    /// the posix and win32 defaults, so this passes on every host.)
    #[test]
    fn path_builtins_accept_malloc_backed_strings() {
        let path = malloc_backed_string(b"/a/b/c.txt");
        assert!(is_string_header_ptr(path));

        let dir = js_path_dirname(path);
        let dir_str = unsafe { string_from_header(dir) }.expect("dirname returns a string");
        assert_eq!(dir_str, "/a/b");

        let base = js_path_basename(path);
        let base_str = unsafe { string_from_header(base) }.expect("basename returns a string");
        assert_eq!(base_str, "c.txt");
    }

    /// Forged pointers (not in the malloc registry, not in an arena) must
    /// still be rejected without dereferencing the candidate header.
    #[test]
    fn forged_unknown_pointer_is_still_rejected() {
        let bogus = 0x0000_7777_0000_1000usize as *const StringHeader;
        assert!(!is_string_header_ptr(bogus));
    }
}

/// The `js_path_posix_*` family must behave identically on every host —
/// it backs the explicit `path.posix.*` namespace, which stays pinned to
/// `/` semantics even on Windows targets.
mod posix_pinned_tests {
    use super::super::*;

    fn s(text: &str) -> *mut StringHeader {
        string_to_js(text)
    }

    fn read(ptr: *mut StringHeader) -> String {
        unsafe { string_from_header(ptr) }.expect("result is a string")
    }

    #[test]
    fn posix_family_is_host_independent() {
        assert_eq!(read(js_path_posix_join(s("a"), s("b"))), "a/b");
        assert_eq!(read(js_path_posix_normalize(s("a/b/../c"))), "a/c");
        assert_eq!(read(js_path_posix_dirname(s("/a/b"))), "/a");
        assert_eq!(read(js_path_posix_basename(s("/a/b.txt"))), "b.txt");
        assert_eq!(read(js_path_posix_extname(s("a/b.txt"))), ".txt");
        assert_eq!(js_path_posix_is_absolute(s("/x")), 1);
        // Win32-style inputs are NOT absolute in the pinned posix namespace,
        // even when the runtime is built for Windows.
        assert_eq!(js_path_posix_is_absolute(s("C:\\x")), 0);
        assert_eq!(js_path_posix_is_absolute(s("relative")), 0);
        assert_eq!(read(js_path_posix_resolve_join(s("/a"), s("b"))), "/a/b");
        assert_eq!(read(js_path_posix_resolve_join(s("a"), s("/b"))), "/b");
        assert_eq!(read(js_path_posix_relative(s("/a/b"), s("/a/c"))), "../c");
    }

    #[test]
    fn posix_to_namespaced_path_is_a_no_op() {
        let value = f64::from_bits(crate::value::JSValue::string_ptr(s("/foo/bar")).bits());
        let out = js_path_posix_to_namespaced_path_value(value);
        let out_ptr = crate::string::js_string_materialize_to_heap(out);
        assert_eq!(
            unsafe { string_from_header(out_ptr) }.as_deref(),
            Some("/foo/bar")
        );
    }
}

/// The DEFAULT `js_path_*` entry points must match the host platform —
/// win32 semantics on Windows (Node: `path === path.win32`), POSIX
/// everywhere else.
mod platform_default_tests {
    use super::super::*;

    fn s(text: &str) -> *mut StringHeader {
        string_to_js(text)
    }

    fn read(ptr: *mut StringHeader) -> String {
        unsafe { string_from_header(ptr) }.expect("result is a string")
    }

    fn obj_field(obj: *mut crate::object::ObjectHeader, name: &str) -> String {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let val = crate::object::js_object_get_field_by_name(obj, key);
        let raw = f64::from_bits(val.bits());
        let ptr = crate::value::js_jsvalue_to_string(raw);
        unsafe { string_from_header(ptr) }.unwrap_or_default()
    }

    #[cfg(windows)]
    #[test]
    fn defaults_use_win32_semantics_on_windows() {
        assert_eq!(read(js_path_sep_get()), "\\");
        assert_eq!(read(js_path_delimiter_get()), ";");

        assert_eq!(read(js_path_join(s("a"), s("b"))), "a\\b");
        assert_eq!(read(js_path_join(s("C:\\a"), s("..\\b"))), "C:\\b");
        assert_eq!(read(js_path_normalize(s("C:/a/b/../c"))), "C:\\a\\c");
        assert_eq!(read(js_path_dirname(s("C:\\a\\b"))), "C:\\a");
        assert_eq!(read(js_path_basename(s("C:\\a\\b.txt"))), "b.txt");
        assert_eq!(read(js_path_extname(s("C:\\a\\b.txt"))), ".txt");

        assert_eq!(js_path_is_absolute(s("C:\\x")), 1);
        assert_eq!(js_path_is_absolute(s("\\x")), 1);
        assert_eq!(js_path_is_absolute(s("C:x")), 0);
        assert_eq!(js_path_is_absolute(s("x")), 0);

        assert_eq!(read(js_path_resolve(s("C:\\a\\..\\b"))), "C:\\b");
        assert_eq!(
            read(js_path_relative(s("C:\\a\\b"), s("C:\\a\\c"))),
            "..\\c"
        );

        let parsed = js_path_parse(s("C:\\dir\\file.txt"));
        assert_eq!(obj_field(parsed, "root"), "C:\\");
        assert_eq!(obj_field(parsed, "dir"), "C:\\dir");
        assert_eq!(obj_field(parsed, "base"), "file.txt");
        assert_eq!(obj_field(parsed, "ext"), ".txt");
        assert_eq!(obj_field(parsed, "name"), "file");

        let value = f64::from_bits(crate::value::JSValue::string_ptr(s("C:\\foo")).bits());
        let out = js_path_to_namespaced_path_value(value);
        let out_ptr = crate::string::js_string_materialize_to_heap(out);
        assert_eq!(
            unsafe { string_from_header(out_ptr) }.as_deref(),
            Some("\\\\?\\C:\\foo")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn defaults_use_posix_semantics_elsewhere() {
        assert_eq!(read(js_path_sep_get()), "/");
        assert_eq!(read(js_path_delimiter_get()), ":");

        assert_eq!(read(js_path_join(s("a"), s("b"))), "a/b");
        assert_eq!(read(js_path_normalize(s("a/b/../c"))), "a/c");
        assert_eq!(read(js_path_dirname(s("/a/b"))), "/a");
        assert_eq!(read(js_path_basename(s("/a/b.txt"))), "b.txt");
        assert_eq!(read(js_path_extname(s("/a/b.txt"))), ".txt");

        assert_eq!(js_path_is_absolute(s("/x")), 1);
        assert_eq!(js_path_is_absolute(s("C:\\x")), 0);

        let parsed = js_path_parse(s("/dir/file.txt"));
        assert_eq!(obj_field(parsed, "root"), "/");
        assert_eq!(obj_field(parsed, "dir"), "/dir");
        assert_eq!(obj_field(parsed, "base"), "file.txt");

        let value = f64::from_bits(crate::value::JSValue::string_ptr(s("/foo")).bits());
        let out = js_path_to_namespaced_path_value(value);
        let out_ptr = crate::string::js_string_materialize_to_heap(out);
        assert_eq!(
            unsafe { string_from_header(out_ptr) }.as_deref(),
            Some("/foo")
        );
    }
}

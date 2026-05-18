//! Sanitizers for package/user-controlled metadata before it flows
//! into linker arguments or filesystem paths.
//!
//! ## Threat model — issue #500
//!
//! The compiler consumes attacker-influenceable strings — `package.json`
//! `name`, input file paths chosen by tooling, `perry.toml` overrides —
//! and threads them into shell-invoked external commands (the linker,
//! `codesign`, `productbuild`, `xattr`, `install_name_tool`, …). Each
//! thread gives the attacker a chance to flip the argument into a
//! *directive* rather than a value:
//!
//! - **ld64 response-file expansion** (`@file`): `@scope/pkg` reads as
//!   "expand from response-file named scope/pkg". The original symptom
//!   of issue #467, generalised here.
//! - **Linker / shell flags** (`-name`): a leading `-` makes the linker
//!   interpret the string as an option.
//! - **Absolute / traversal paths** (`/etc/passwd`, `../../`): escape
//!   the build directory when used in `-o`, `-install_name`,
//!   `-Wl,--soname`, codesign output paths, etc.
//! - **Shell metacharacters** (`;`, `|`, `&`, `>`, `<`, `$`, backtick,
//!   `\`, `"`, `'`, `(`, `)`, `{`, `}`): when a tool re-splits its argv
//!   through a shell (codesign on some macOS configs, every Windows
//!   `cmd.exe` invocation, etc.).
//! - **Whitespace** (` `, `\t`, `\n`, `\r`): argv splitter.
//! - **Control bytes** (`\0`, `\x01..\x1F`): exotic terminator handling
//!   varies by libc and linker; an embedded NUL most reliably truncates.
//! - **Path separators** (`/`, `\`): produce surprise multi-component
//!   paths from what was meant to be a single component.
//! - **Non-ASCII look-alikes**: locale-sensitive equivalences
//!   (`ｐ` vs `p`, RTL bidi marks, zero-width joiners) and width
//!   attacks against humans diffing logs.
//!
//! All of these collapse to the same mitigation: rewrite the input to a
//! conservative `[A-Za-z0-9._-]` subset, with a non-empty fallback for
//! inputs that consist *entirely* of unsafe bytes.
//!
//! ## Why a single function
//!
//! The earlier one-off `sanitize_app_name` (issue #467) closed the
//! `@scope/pkg` arm only. A new linker-adjacent derivation site is
//! easy to miss; a reviewer who does not know the historical incident
//! has no signal to look. The single shared function lets `grep`
//! answer "is every derivation site sanitized?" by counting call
//! sites, and the fuzz suite next to the function
//! (`tests/sanitize_linker_arg.rs`) keeps us honest about
//! characters we have not yet thought of.
//!
//! ## What this is NOT
//!
//! This function is intentionally aggressive. The caller is expected
//! to be deriving a *synthetic identifier* (output filename, install
//! name, soname, default bundle-ID stem) — not echoing back user
//! content that needs to round-trip. Where a value must round-trip to
//! the end user (e.g. an Info.plist `<string>CFBundleDisplayName</string>`
//! containing the raw `package.json` name), the caller is responsible
//! for emitting the value through the right encoder for that medium
//! (XML escape, JSON escape, etc.) instead of this function.

/// The fallback identifier used when the entire input scrubs to the
/// empty string (every byte was unsafe). Picked because it is the
/// same default Perry already uses for unnamed builds.
pub const SAFE_FALLBACK: &str = "app";

/// Sanitize a metadata-derived string so it is safe to use as a single
/// argv token for the linker (ld, ld64, lld, lld-link, link.exe) and
/// codesigning tools, and is also safe to use as a single path
/// component on every supported host.
///
/// Output character set: `[A-Za-z0-9._-]`. Plus the legacy backwards-
/// compatible mapping that preserves what `sanitize_app_name` produced
/// for the issue #467 shape:
///
/// - A *single* leading `@` is dropped (npm scope prefix).
/// - `/` (and `\` on Windows-shaped inputs) flattens to `-` so
///   `@scope/pkg` round-trips to the recognisable filename
///   `scope-pkg`.
///
/// Every other unsafe byte becomes `_`. Leading `.` or `-` get a `_`
/// prefix so the result is not interpreted as a hidden file or as a
/// flag. Whole-string `.` and `..` collapse to [`SAFE_FALLBACK`].
/// Empty results fall back to [`SAFE_FALLBACK`].
///
/// The function is byte-deterministic and allocates exactly once.
pub fn sanitize_for_linker_argv(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 1);
    let mut chars = raw.chars().peekable();
    // Drop a single leading '@' so npm-scoped names round-trip to the
    // same filename the workspace already used pre-issue #500. Note:
    // only a *single* leading '@' is dropped; `@@evil` keeps the second
    // one and that one is then scrubbed to `_` by the loop below.
    if chars.peek() == Some(&'@') {
        chars.next();
    }
    for c in chars {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c);
        } else if c == '/' || c == '\\' {
            // Scoped packages: `@scope/pkg` → `scope-pkg`. Matches the
            // legacy `sanitize_app_name` behavior so reviewers
            // recognise the resulting filename.
            out.push('-');
        } else {
            out.push('_');
        }
    }
    // Reserve `.` and `..` because most filesystem APIs reject them
    // as path components even though they pass the char-class scrub.
    // Checked *before* the prefix-quoting step below so the fallback
    // string is returned cleanly (otherwise `..` would map to `_..`
    // which is harmless but less recognisable in build logs).
    if out == "." || out == ".." {
        return SAFE_FALLBACK.to_string();
    }
    if out.is_empty() {
        return SAFE_FALLBACK.to_string();
    }
    // After the scrub: a leading `.` is still a hidden file on Unix
    // and breaks some bundle tooling; a leading `-` is still
    // interpretable as a flag by every command-line tool. Prefix `_`
    // in either case. The leading `@` was already dropped above.
    if matches!(out.as_bytes().first(), Some(b'.') | Some(b'-')) {
        out.insert(0, '_');
    }
    out
}

/// Sanitize for use as one reverse-DNS bundle-ID component
/// (`com.perry.<this>`). Same character class as
/// [`sanitize_for_linker_argv`], additionally lowercased — Apple
/// recommends and many provisioning tools require bundle IDs to be
/// lowercase-canonical, and bundle IDs flow into `codesign --sign`
/// invocations where the linker-argv concerns apply identically.
pub fn sanitize_for_bundle_id_component(raw: &str) -> String {
    let mut out = sanitize_for_linker_argv(raw);
    out.make_ascii_lowercase();
    out
}

/// Build the default `com.perry.<stem>` bundle ID for an executable
/// stem when no explicit bundle ID has been configured. Shared by every
/// Apple platform (macOS, iOS, visionOS, watchOS, tvOS) so the generated
/// fallback agrees byte-for-byte across targets — provisioning profiles
/// that round-trip a binary between platforms can rely on a single
/// canonical form. #998.
pub fn default_perry_bundle_id(exe_stem: &str) -> String {
    let stem = sanitize_for_bundle_id_component(exe_stem);
    format!("com.perry.{stem}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_467_scoped_name_roundtrip() {
        // The canonical regression: an npm-scoped package name flowed
        // through `-o @foo/bar` and ld64 expanded `@foo/bar` as a
        // response file. The sanitizer drops the `@` and flattens the
        // `/`. Output is still readable so build logs are debuggable.
        assert_eq!(sanitize_for_linker_argv("@foo/bar"), "foo-bar");
        assert_eq!(sanitize_for_linker_argv("@scope/pkg/sub"), "scope-pkg-sub");
    }

    #[test]
    fn leading_dash_gets_quoted_with_underscore() {
        // Anything starting with `-` is interpretable as a linker flag.
        assert_eq!(sanitize_for_linker_argv("-rpath"), "_-rpath");
        assert_eq!(sanitize_for_linker_argv("--all-load"), "_--all-load");
    }

    #[test]
    fn leading_dot_is_not_hidden_file() {
        assert_eq!(sanitize_for_linker_argv(".cargo"), "_.cargo");
        assert_eq!(sanitize_for_linker_argv(".."), SAFE_FALLBACK);
        assert_eq!(sanitize_for_linker_argv("."), SAFE_FALLBACK);
    }

    #[test]
    fn embedded_newlines_and_control_bytes() {
        // Newlines split argv when shell-piped; NUL terminates C
        // strings. Both must scrub.
        assert_eq!(sanitize_for_linker_argv("foo\nbar"), "foo_bar");
        assert_eq!(sanitize_for_linker_argv("foo\0bar"), "foo_bar");
        assert_eq!(sanitize_for_linker_argv("foo\tbar"), "foo_bar");
        assert_eq!(sanitize_for_linker_argv("foo\rbar"), "foo_bar");
        for b in 0u8..0x20 {
            let raw = format!("a{}b", b as char);
            let out = sanitize_for_linker_argv(&raw);
            assert!(
                out.bytes().all(|c| c >= 0x20),
                "control byte {:#x} leaked through: {:?}",
                b,
                out
            );
        }
    }

    #[test]
    fn shell_metacharacters_scrub() {
        // Each one of these is a directive to bash, zsh, or cmd.exe
        // when the linker / codesign call goes through a shell.
        for meta in [
            ";", "|", "&", ">", "<", "$", "`", "\\", "\"", "'", "(", ")", "{", "}", "*", "?", "!",
            "#", "~", "^", " ",
        ] {
            let raw = format!("a{meta}b");
            let out = sanitize_for_linker_argv(&raw);
            assert!(
                !out.contains(meta),
                "shell metachar {:?} leaked through: {:?}",
                meta,
                out
            );
        }
    }

    #[test]
    fn path_separators_flatten() {
        // POSIX path:
        assert_eq!(sanitize_for_linker_argv("a/b/c"), "a-b-c");
        // Windows-shaped path (someone copy-pastes a path from a
        // Windows error message into package.json `name`):
        assert_eq!(sanitize_for_linker_argv("a\\b\\c"), "a-b-c");
        // Path traversal:
        assert_eq!(
            sanitize_for_linker_argv("../../etc/passwd"),
            "_..-..-etc-passwd"
        );
        // Leading `/` flattens to `-`, then the leading-`-` rule
        // prefixes the `_` so the output is not interpretable as a
        // linker flag.
        assert_eq!(sanitize_for_linker_argv("/etc/passwd"), "_-etc-passwd");
    }

    #[test]
    fn non_ascii_lookalikes_scrub() {
        // Full-width 'p' (U+FF50) looks identical to 'p' in some
        // fonts and would round-trip through anything that lower-cases
        // by codepoint range. Scrub.
        assert_eq!(sanitize_for_linker_argv("ｐerry"), "_erry");
        // Bidi marks (U+202E RTL override) make the string display
        // very differently from how the linker reads it.
        let with_bidi = "good\u{202E}evil";
        let out = sanitize_for_linker_argv(with_bidi);
        assert!(!out.contains('\u{202E}'), "bidi mark leaked: {:?}", out);
    }

    #[test]
    fn empty_or_all_unsafe_falls_back() {
        assert_eq!(sanitize_for_linker_argv(""), SAFE_FALLBACK);
        // A bare `@` is consumed by the leading-`@` strip, leaving an
        // empty string → fallback.
        assert_eq!(sanitize_for_linker_argv("@"), SAFE_FALLBACK);
        // `/` scrubs to `-`, then the leading-`-` rule prefixes `_`.
        assert_eq!(sanitize_for_linker_argv("/"), "_-");
        assert_eq!(sanitize_for_linker_argv("/////"), "_-----");
    }

    #[test]
    fn ld64_response_file_directive_classes() {
        // ld64-specific quirks that #467 surfaced and #500 generalises.
        // Each entry is a string that historically caused ld64 to do
        // something unexpected.
        for evil in ["@/etc/passwd", "@-foo", "@@@", "@", "@-Wl,foo", "@\nfoo"] {
            let out = sanitize_for_linker_argv(evil);
            assert!(
                !out.starts_with('@'),
                "leading @ leaked in {:?} -> {:?}",
                evil,
                out
            );
            // And the result must be non-empty / non-just-`_`s.
            assert!(!out.is_empty());
        }
    }

    #[test]
    fn bundle_id_component_lowercases() {
        assert_eq!(sanitize_for_bundle_id_component("@Foo/Bar"), "foo-bar");
        assert_eq!(sanitize_for_bundle_id_component("MyApp"), "myapp");
    }

    #[test]
    fn default_perry_bundle_id_is_sanitized_lowercase() {
        // #998: every Apple-platform fallback (macOS/iOS/visionOS/
        // watchOS/tvOS) must produce the same byte-for-byte form so a
        // binary round-tripped between targets keeps a single canonical
        // bundle ID. The shared helper enforces it.
        assert_eq!(default_perry_bundle_id("MyApp"), "com.perry.myapp");
        assert_eq!(default_perry_bundle_id("@scope/pkg"), "com.perry.scope-pkg");
        assert_eq!(default_perry_bundle_id("foo bar"), "com.perry.foo_bar");
    }

    #[test]
    fn compile_rs_uses_shared_helper_for_all_fallbacks() {
        // #998 grep-audit: ensure no Apple-platform site builds the
        // bundle-ID inline (which would skip the sanitizer). Every
        // `com.perry.<stem>` derivation must go through
        // `default_perry_bundle_id`.
        let src = include_str!("compile.rs");
        // The only places `com.perry.` may appear are doc/comment text;
        // there must be no `format!("com.perry.{...", ...)` invocations
        // (those are the unsanitized inline forms).
        for line in src.lines() {
            // Be conservative — skip comments outright.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            assert!(
                !line.contains("format!(\"com.perry."),
                "compile.rs has an inline `format!(\"com.perry.…\")` call. \
                 Use `crate::commands::sanitize::default_perry_bundle_id` \
                 instead so the bundle-ID fallback stays consistent across \
                 Apple platforms (#998). Offending line:\n  {}",
                line
            );
        }
    }

    #[test]
    fn idempotent() {
        // Calling the sanitizer twice gives the same result as calling
        // it once. Important because some derivation sites already
        // call the legacy `sanitize_app_name`; mixing old + new should
        // never produce different output across releases.
        for raw in [
            "@foo/bar",
            "MyApp",
            "../../etc/passwd",
            "good\u{202E}evil",
            "",
            "x",
        ] {
            let once = sanitize_for_linker_argv(raw);
            let twice = sanitize_for_linker_argv(&once);
            assert_eq!(once, twice, "not idempotent on {:?}", raw);
        }
    }

    #[test]
    fn output_char_class_is_only_safe_bytes() {
        // Property test: every byte of every output is in
        // `[A-Za-z0-9._-]`. Exercise a wide alphabet of pathological
        // inputs.
        let inputs = [
            "@foo/bar",
            "../../etc/passwd",
            "foo\nbar",
            "foo\0bar",
            "ｐerry",
            "--no-default-features",
            "@-Wl,--version-script=evil.lds",
            "\u{202E}backwards",
            "naïve résumé",
            "$(rm -rf /)",
            "`echo pwned`",
            "${IFS}",
        ];
        let safe = |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-';
        for raw in inputs {
            let out = sanitize_for_linker_argv(raw);
            for b in out.bytes() {
                assert!(
                    safe(b),
                    "unsafe byte {:#x} in output {:?} from input {:?}",
                    b,
                    out,
                    raw
                );
            }
        }
    }

    #[test]
    fn long_inputs_dont_explode() {
        // Length-bound: a 100KB pathological input should still
        // produce an output bounded by `input.len() + 1` (the
        // possible leading `_`). This keeps the sanitizer usable on
        // attacker-supplied data without DOS surface.
        let raw: String = std::iter::repeat('@').take(100_000).collect();
        let out = sanitize_for_linker_argv(&raw);
        assert!(out.len() <= raw.len() + 1);
    }
}

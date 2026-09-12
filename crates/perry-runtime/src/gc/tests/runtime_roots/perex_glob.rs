//! Glob consumers of the one Perex compiler/matcher and program owner.
use super::*;
use crate::regex::perex_glob::{GlobProgram, NativeText};
use crate::regex::perex_memory::MemoryBudget;
use crate::regex::perex_runtime::EngineError;
use crate::value::{js_nanbox_get_pointer, js_nanbox_pointer, js_nanbox_string, TAG_FALSE};
use perex::binding::BoundSubject;
use perex::{executor::ExecError, Budget};

fn text<'s>(scope: &'s RuntimeHandleScope, text: &str) -> RuntimeHandle<'s> {
    scope.root_nanbox_f64(js_nanbox_string(crate::string::js_string_from_bytes(
        text.as_ptr(),
        text.len() as u32,
    ) as i64))
}
fn bytes(value: f64) -> Vec<u8> {
    let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
    let (p, n) = crate::string::str_bytes_from_jsvalue(value, &mut short).unwrap();
    unsafe { std::slice::from_raw_parts(p, n as usize).to_vec() }
}
fn set(owner: &RuntimeHandle<'_>, name: &[u8], value: f64) {
    let scope = RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = crate::string::canonical_key(name);
    assert!(crate::proxy::create_data_property(
        owner.get_nanbox_f64(),
        js_nanbox_string(key as i64),
        value.get_nanbox_f64()
    ));
}

#[test]
fn perex_glob_borrows_native_text_and_relocates_its_gc_program() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let scope = RuntimeHandleScope::new();
    let input = "ab".repeat(32) + "z";
    let subject = BoundSubject::new(NativeText(&input)).unwrap();
    assert_eq!(
        subject
            .with_view(|s| s.ascii_bytes().unwrap().as_ptr())
            .unwrap(),
        input.as_ptr()
    );
    let program = GlobProgram::new(&scope, r"^(?!(?:bad)(?:/|$))(?:a|ab)*z(?![\s\S])").unwrap();
    let before = program.program_words_address();
    let live = external_side_live_bytes();
    let memory = MemoryBudget::new(1 << 20);
    let mut polls = 0;
    assert!(program
        .test_with(
            &subject,
            &mut Budget::new(1_000_000),
            &memory,
            1,
            &mut || {
                polls += 1;
                gc_collect_minor();
                Ok(())
            }
        )
        .unwrap());
    assert!(polls > 2);
    assert_ne!(program.program_words_address(), before);
    assert_eq!(
        subject
            .with_view(|s| s.ascii_bytes().unwrap().as_ptr())
            .unwrap(),
        input.as_ptr()
    );
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), live);
    assert!(matches!(
        program.test_with(&subject, &mut Budget::new(0), &memory, 1, &mut || Ok(())),
        Err(EngineError::Execution(ExecError::WorkLimit))
    ));
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), live);
    let mut calls = 0;
    assert!(matches!(
        program.test_with(&subject, &mut Budget::new(100_000), &memory, 1, &mut || {
            calls += 1;
            if calls == 3 {
                Err(EngineError::Cancelled)
            } else {
                Ok(())
            }
        }),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(memory.live_bytes(), 0);
    assert_eq!(external_side_live_bytes(), live);
}

#[test]
fn perex_glob_path_modes_match_original_unicode_and_absolute_end() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::perex_public::register_host_roots();
    for (path, pattern, win32, expected) in [
        ("src/é.ts", "src/é.ts", false, 1),
        ("src/😀.ts", "src/{😀,é}.ts", false, 1),
        ("src/a.ts", "src/*.ts", false, 1),
        ("src/a.ts\n", "src/*.ts", false, 0),
        ("src/a\nb.ts", "src/*.ts", false, 1),
        ("src/a.ts\u{2028}", "src/*.ts", false, 0),
        (r"src\é.ts", "src/é.ts", true, 1),
        (r"src\nested\a.ts", "src/**/*.ts", true, 1),
        (r"src\nested\a.ts", "src/*.ts", true, 0),
        (r"src\a.ts", "src/*.ts", false, 0),
    ] {
        let scope = RuntimeHandleScope::new();
        let path_value = text(&scope, path);
        let pattern_value = text(&scope, pattern);
        let value = if win32 {
            crate::path::js_path_win32_matches_glob(
                js_nanbox_get_pointer(path_value.get_nanbox_f64()) as *const _,
                js_nanbox_get_pointer(pattern_value.get_nanbox_f64()) as *const _,
            )
        } else {
            crate::path::js_path_posix_matches_glob(
                js_nanbox_get_pointer(path_value.get_nanbox_f64()) as *const _,
                js_nanbox_get_pointer(pattern_value.get_nanbox_f64()) as *const _,
            )
        };
        assert_eq!(value, expected, "{path:?} / {pattern:?} win32={win32}");
        assert_eq!(bytes(path_value.get_nanbox_f64()), path.as_bytes());
    }
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("perex-glob-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        for name in ["keep.txt", "skip.txt", "other.md"] {
            std::fs::write(path.join(name), b"test").unwrap();
        }
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
extern "C" fn exclude(_: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let arg = scope.root_nanbox_f64(arg);
    gc_collect_minor();
    let pattern = text(&scope, "*.txt");
    assert!(crate::fs::bun_glob_matches(pattern.get_nanbox_f64(), arg.get_nanbox_f64()).unwrap());
    gc_collect_minor();
    f64::from_bits(if bytes(arg.get_nanbox_f64()) == b"skip.txt" {
        crate::value::TAG_TRUE
    } else {
        TAG_FALSE
    })
}
extern "C" fn throw_exclude(_: *const crate::closure::ClosureHeader, _: f64) -> f64 {
    gc_collect_minor();
    crate::exception::js_throw(991.0)
}

#[test]
fn perex_glob_filesystem_callbacks_reenter_collect_and_throw_without_lost_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    super::perex_public::register_host_roots();
    let directory = Directory::new();
    let scope = RuntimeHandleScope::new();
    let pattern = text(&scope, "*.txt");
    let cwd = text(&scope, directory.0.to_str().unwrap());
    let options = scope.root_nanbox_f64(js_nanbox_pointer(
        crate::object::js_object_alloc(0, 4) as i64
    ));
    set(&options, b"cwd", cwd.get_nanbox_f64());
    crate::closure::js_register_closure_arity(exclude as *const u8, 1);
    let callback = scope
        .root_nanbox_f64(js_nanbox_pointer(
            crate::closure::js_closure_alloc(exclude as *const u8, 0) as i64,
        ));
    set(&options, b"exclude", callback.get_nanbox_f64());
    let before = callback.get_nanbox_f64().to_bits();
    let live = external_side_live_bytes();
    let result =
        crate::fs::js_fs_glob_sync_options(pattern.get_nanbox_f64(), options.get_nanbox_f64());
    let result = scope.root_raw_mut_ptr(result.to_bits() as *mut crate::array::ArrayHeader);
    assert_eq!(crate::array::js_array_length(result.get_raw_const_ptr()), 1);
    assert_eq!(
        bytes(crate::array::js_array_get_f64(
            result.get_raw_const_ptr(),
            0
        )),
        b"keep.txt"
    );
    assert_ne!(callback.get_nanbox_f64().to_bits(), before);
    assert_eq!(external_side_live_bytes(), live);
    crate::closure::js_register_closure_arity(throw_exclude as *const u8, 1);
    let callback = scope.root_nanbox_f64(js_nanbox_pointer(crate::closure::js_closure_alloc(
        throw_exclude as *const u8,
        0,
    ) as i64));
    set(&options, b"exclude", callback.get_nanbox_f64());
    let roots = RuntimeHandleScope::active_len_for_tests();
    let error = crate::exception::catch_js_throw(|| {
        crate::fs::js_fs_glob_sync_options(pattern.get_nanbox_f64(), options.get_nanbox_f64())
    })
    .unwrap_err();
    assert_eq!(error, 991.0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), roots);
    assert_eq!(external_side_live_bytes(), live);
    let empty = text(&scope, "none.*");
    let result =
        crate::fs::js_fs_glob_sync_options(empty.get_nanbox_f64(), options.get_nanbox_f64());
    assert_eq!(
        crate::array::js_array_length(result.to_bits() as *const _),
        0
    );
}

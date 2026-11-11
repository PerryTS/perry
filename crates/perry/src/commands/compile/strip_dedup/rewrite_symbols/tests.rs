//! Real object-tool acceptance tests; LLVM tools are optional outside the tool-equipped lane.
#![cfg(unix)]

use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    dir: PathBuf,
    tools: PathBuf,
}

impl Fixture {
    fn new() -> Option<Self> {
        let declared = std::env::var_os("LLVM_TEST_TOOL_DIR").map(PathBuf::from);
        let tools = declared.clone().unwrap_or_default();
        for tool in ["clang", "llvm-nm", "llvm-readobj", "llvm-objcopy"] {
            if !Command::new(tools.join(tool))
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
            {
                // A lane that declares its tool directory must not pass this
                // suite having exercised nothing.
                assert!(
                    declared.is_none(),
                    "LLVM_TEST_TOOL_DIR is set but {tool} is unusable"
                );
                eprintln!("unavailable: {tool}; real symbol-rewrite test not exercised");
                return None;
            }
        }
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "perry-rewrite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        eprintln!("rewrite fixture: {}", dir.display());
        Some(Self { dir, tools })
    }

    fn run(&self, tool: &str, args: &[&std::ffi::OsStr]) -> String {
        let out = Command::new(self.tools.join(tool))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{tool}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn object(&self, name: &str, target: &str, source: &str) -> PathBuf {
        let input = self.dir.join(format!("{name}.c"));
        let object = self.dir.join(format!("{name}.o"));
        std::fs::write(&input, source).unwrap();
        self.run(
            "clang",
            &[
                "-target".as_ref(),
                target.as_ref(),
                "-c".as_ref(),
                input.as_os_str(),
                "-o".as_ref(),
                object.as_os_str(),
            ],
        );
        object
    }

    fn wrapper(&self) -> PathBuf {
        let path = self.dir.join("count-objcopy");
        // Single quotes protect spaces and shell metacharacters in tool/fixture paths.
        let quote = |p: &Path| format!("'{}'", p.to_str().unwrap().replace('\'', "'\\''"));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf 'RUN\\n' >> {}\nprintf '%s\\n' \"$@\" >> {}\nexec {} \"$@\"\n",
                quote(&self.dir.join("calls")),
                quote(&self.dir.join("argv")),
                quote(&self.tools.join("llvm-objcopy"))
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(self.dir.join("calls"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Replay the recorded argv and assert every objcopy call stayed inside the
    /// production argument budget, counted the way `rewrite_member_symbols`
    /// counts it (objcopy and member path included in every batch).
    fn assert_bounded_argv(&self, objcopy: &Path, member: &Path) {
        let args = std::fs::read_to_string(self.dir.join("argv")).unwrap();
        // `rewrite_member_symbols` counts the objcopy path as the base of every
        // batch and adds each argument, the member path included, as it goes.
        let base = argument_bytes(objcopy.as_os_str());
        let mut cost = base;
        for arg in args.lines() {
            cost += argument_bytes(std::ffi::OsStr::new(arg));
            if arg == member.to_str().unwrap() {
                assert!(cost <= 8192, "command arguments exceeded budget: {cost}");
                cost = base;
            }
        }
        assert_eq!(cost, base);
    }

    fn symbols(&self, object: &Path) -> BTreeMap<String, String> {
        let text = self.run("llvm-nm", &["--format=posix".as_ref(), object.as_os_str()]);
        std::fs::write(object.with_extension("nm"), &text).unwrap();
        eprintln!("{} symbols:\n{text}", object.display());
        text.lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                Some((words.next()?.into(), words.next()?.into()))
            })
            .collect()
    }

    fn relocations(&self, object: &Path) -> String {
        let text = self.run(
            "llvm-readobj",
            &["--relocations".as_ref(), object.as_os_str()],
        );
        std::fs::write(object.with_extension("relocations"), &text).unwrap();
        eprintln!("{text}");
        text
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("LLVM_TEST_KEEP_ARTIFACTS").is_none() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

const SOURCE: &str = r#"
int __rust_alloc = 1;
int __rust_dealloc = 2;
int rust_eh_personality = 3;
int rust_begin_unwind = 4;
int personality_ref __asm__("DW.ref.rust_eh_personality") = 5;
int *references[] = { &__rust_alloc, &__rust_dealloc, &rust_eh_personality,
                      &rust_begin_unwind, &personality_ref };
"#;

fn selected() -> Vec<String> {
    [
        "DW.ref.rust_eh_personality",
        "__rust_alloc",
        "__rust_dealloc",
        "rust_begin_unwind",
        "rust_eh_personality",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

#[test]
fn elf_batches_allocators_without_localizing_panic_or_unwind_relocations() {
    let Some(f) = Fixture::new() else { return };
    let object = f.object("elf", "x86_64-unknown-linux-gnu", SOURCE);
    let before = f.relocations(&object);
    rewrite_member_symbols(
        &f.wrapper(),
        &object,
        "elf.o",
        &selected(),
        false,
        "wrapper_a",
        7,
    )
    .unwrap();
    let symbols = f.symbols(&object);
    assert_eq!(symbols["__rust_alloc"], "d");
    assert_eq!(symbols["__rust_dealloc"], "d");
    let after = f.relocations(&object);
    for symbol in [
        "DW.ref.rust_eh_personality",
        "rust_begin_unwind",
        "rust_eh_personality",
    ] {
        assert_eq!(symbols[symbol], "D");
        assert!(before.contains(symbol) && after.contains(symbol));
    }
    assert!(
        after.contains(".data"),
        "allocator relocations still target data"
    );
    assert_eq!(f.calls(), 1, "one objcopy per small member, not per symbol");
}

#[test]
fn coff_batches_renames_with_original_indices_and_distinct_archive_member_names() {
    let Some(f) = Fixture::new() else { return };
    let mut seen = std::collections::BTreeSet::new();
    for (name, tag, member) in [
        ("coff_a", "archive_a", 7),
        ("coff_b", "archive_b", 7),
        ("coff_c", "archive_a", 8),
    ] {
        let object = f.object(name, "x86_64-pc-windows-msvc", SOURCE);
        rewrite_member_symbols(&f.wrapper(), &object, name, &selected(), true, tag, member)
            .unwrap();
        let symbols = f.symbols(&object);
        let relocations = f.relocations(&object);
        for (index, original) in selected().iter().enumerate() {
            let renamed = format!("__perry_wrapper_local_{tag}_{member}_{index}");
            assert!(!symbols.contains_key(original));
            assert_eq!(symbols[&renamed], "D");
            assert!(relocations.contains(&renamed));
            assert!(seen.insert(renamed));
        }
    }
    assert_eq!(seen.len(), 15);
    assert_eq!(f.calls(), 3, "one objcopy for each COFF member");
}

#[test]
fn empty_and_elf_panic_only_members_do_not_launch_objcopy() {
    let Some(f) = Fixture::new() else { return };
    let object = f.object("skip", "x86_64-unknown-linux-gnu", SOURCE);
    let before = std::fs::read(&object).unwrap();
    let wrapper = f.wrapper();
    rewrite_member_symbols(&wrapper, &object, "skip", &[], false, "a", 0).unwrap();
    let panic = [
        "DW.ref.rust_eh_personality",
        "rust_eh_personality",
        "rust_begin_unwind",
    ]
    .map(String::from);
    rewrite_member_symbols(&wrapper, &object, "skip", &panic, false, "a", 0).unwrap();
    assert_eq!(f.calls(), 0);
    assert_eq!(std::fs::read(object).unwrap(), before);
}

#[test]
fn long_symbol_lists_are_batched_but_keep_command_lines_bounded() {
    let Some(f) = Fixture::new() else { return };
    let symbols: Vec<String> = (0..90)
        .map(|i| format!("allocator_{i}_{}", "x".repeat(180)))
        .collect();
    let source = symbols
        .iter()
        .map(|s| format!("int {s}=1;\n"))
        .collect::<String>();
    let object = f.object("large", "x86_64-unknown-linux-gnu", &source);
    let wrapper = f.wrapper();
    rewrite_member_symbols(&wrapper, &object, "large", &symbols, false, "a", 0).unwrap();
    let rewritten = f.symbols(&object);
    assert!(symbols
        .iter()
        .all(|s| rewritten.get(s).map(String::as_str) == Some("d")));
    let calls = f.calls();
    assert!(
        calls > 1 && calls < 10,
        "bounded batches, not {calls} processes"
    );
    // The log retains every actual argument; the member path terminates a batch.
    f.assert_bounded_argv(&wrapper, &object);
}

#[test]
fn objcopy_failure_identifies_first_symbol_and_member_without_modifying_input() {
    let Some(f) = Fixture::new() else { return };
    let object = f.dir.join("invalid.o");
    std::fs::write(&object, b"not an object").unwrap();
    let error = rewrite_member_symbols(
        &f.wrapper(),
        &object,
        "invalid.o",
        &["__rust_alloc".into()],
        false,
        "a",
        0,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .starts_with("failed to rewrite __rust_alloc in invalid.o:"));
    assert_eq!(std::fs::read(object).unwrap(), b"not an object");
    assert_eq!(f.calls(), 1);
}

#[test]
fn oversized_single_symbol_keeps_neighboring_rewrites() {
    let Some(f) = Fixture::new() else { return };
    let names = vec![
        "neighbor_before".to_owned(),
        format!("oversized_{}", "x".repeat(5000)),
        "neighbor_after".to_owned(),
    ];
    let mut source = names
        .iter()
        .map(|name| format!("int {name}=1;\n"))
        .collect::<String>();
    source += &format!(
        "int *refs[]={{ {} }};\n",
        names
            .iter()
            .map(|name| format!("&{name}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let object = f.object("singleton", "x86_64-unknown-linux-gnu", &source);
    rewrite_member_symbols(
        &f.wrapper(),
        &object,
        "singleton.o",
        &names,
        false,
        "archive",
        3,
    )
    .unwrap();
    let symbols = f.symbols(&object);
    for name in &names {
        assert_eq!(symbols[name], "d");
    }
    f.relocations(&object);
    assert_eq!(f.calls(), 3);
    // An indivisible argument still reaches objcopy between its neighboring batches.
    let args = std::fs::read_to_string(f.dir.join("argv")).unwrap();
    assert!(args.lines().any(|arg| arg == names[1]));
}

#[test]
fn coff_batch_splits_keep_original_symbol_and_member_indices() {
    let Some(f) = Fixture::new() else { return };
    let names = (0..90)
        .map(|index| format!("coff_symbol_{index}_{}", "x".repeat(180)))
        .collect::<Vec<_>>();
    let mut source = names
        .iter()
        .map(|name| format!("int {name}=1;\n"))
        .collect::<String>();
    source += &format!(
        "int *refs[]={{ {} }};\n",
        names
            .iter()
            .map(|name| format!("&{name}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let object = f.object("coff_split", "x86_64-pc-windows-msvc", &source);
    let wrapper = f.wrapper();
    rewrite_member_symbols(
        &wrapper,
        &object,
        "coff_split.o",
        &names,
        true,
        "archive_tag",
        19,
    )
    .unwrap();
    let symbols = f.symbols(&object);
    let relocations = f.relocations(&object);
    for (index, original) in names.iter().enumerate() {
        let renamed = format!("__perry_wrapper_local_archive_tag_19_{index}");
        assert!(!symbols.contains_key(original));
        assert_eq!(symbols[&renamed], "D");
        assert!(relocations.contains(&format!("{renamed} (")));
    }
    f.assert_bounded_argv(&wrapper, &object);
}

//! wasm32 lowering for the standalone WASI target (#11375, #11378).
//!
//! Only compiled with the off-by-default `target-wasi` feature, and only ever
//! applied to a module whose triple is wasm32 — no other target's output can
//! reach anything in here.

mod abi_adapt;
mod closure_abi;

/// The wasm32 lowering applied to one finished module's IR text.
pub(crate) fn lower_module(ir: &str) -> String {
    let (ir, closure_bodies) = closure_abi::retype_closure_bodies(ir);
    rename_entry_point(&protect_setjmp_locals(&abi_adapt::adapt_runtime_abi(
        &ir,
        &closure_bodies,
    )))
}

// The wasm fallback uses shadow roots, rather than RS4GC. Keep mutable
// locals in memory across a nonlocal return (mem2reg would otherwise use
// the pre-try SSA value), and prevent inlining a returns-twice frame.
fn protect_setjmp_locals(ir: &str) -> String {
    let mut out = String::with_capacity(ir.len());
    let mut lines = ir.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("declare i32 @setjmp(") {
            out.push_str("declare i32 @setjmp(ptr) returns_twice\n");
        } else if line.starts_with("define ") {
            let mut function = vec![line];
            for body_line in lines.by_ref() {
                function.push(body_line);
                if body_line == "}" {
                    break;
                }
            }
            let has_setjmp = function
                .iter()
                .any(|line| line.contains("call i32 @setjmp("));
            let slots: Vec<String> = function
                .iter()
                .filter_map(|line| {
                    line.split_once(" = alloca ")
                        .map(|(name, _)| format!("ptr {}", name.trim()))
                })
                .collect();
            for (index, line) in function.into_iter().enumerate() {
                let mut line = line.to_string();
                if has_setjmp {
                    if index == 0 {
                        line = line
                            .replace(" alwaysinline", "")
                            .replace(" {", " noinline {");
                    } else if line
                        .split(',')
                        .any(|part| slots.iter().any(|slot| part.trim() == slot))
                    {
                        line = line.replace(" = load atomic ", " = load atomic volatile ");
                        if !line.contains(" = load atomic ") {
                            line = line.replace(" = load ", " = load volatile ");
                        }
                        if line.trim_start().starts_with("store ") {
                            line = line.replacen("store ", "store volatile ", 1);
                        }
                    }
                }
                out.push_str(&line);
                out.push('\n');
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The emitted object without the NUL terminator inkwell's
/// `MemoryBuffer::as_slice` includes (LLVM guarantees one readable byte past
/// the end of a buffer). ELF/Mach-O/COFF readers locate everything through
/// their headers and never see it; a wasm object is a flat sequence of
/// sections, so wasm-ld reads the extra byte as the start of another section
/// and aborts ("malformed uleb128").
pub(crate) fn trim_object(mut object: Vec<u8>) -> Vec<u8> {
    if object.starts_with(b"\0asm") && object.last() == Some(&0) {
        object.pop();
    }
    object
}

/// wasi-libc's `_start` calls `__main_void` (or `__main_argc_argv`), the
/// names clang gives a C `main` on wasm; a plain `@main` is never reached and
/// the program traps in the weak default. Rename the generated entry point.
fn rename_entry_point(ir: &str) -> String {
    let Some(def) = ir
        .lines()
        .find(|l| l.starts_with("define ") && l.contains(" @main("))
    else {
        return ir.to_string();
    };
    let takes_args = !def.contains(" @main()");
    let to = if takes_args {
        "@__main_argc_argv"
    } else {
        "@__main_void"
    };
    let mut out = String::with_capacity(ir.len() + 16);
    let mut rest = ir;
    while let Some(at) = rest.find("@main") {
        let after = &rest[at + 5..];
        // Only the whole symbol `@main`, not `@main_something`.
        let boundary = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$'));
        out.push_str(&rest[..at]);
        out.push_str(if boundary { to } else { "@main" });
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sjlj_keeps_mutated_stack_locals_but_does_not_retype_atomic_globals() {
        let ir = "declare i32 @setjmp(ptr)\ndefine void @f() optsize {\nentry:\n  %slot = alloca i64\n  store i64 1, ptr %slot\n  %r = call i32 @setjmp(ptr %buf)\n  %v = load i64, ptr %slot\n  %g = load atomic i64, ptr @global monotonic, align 8\n  ret void\n}\n";
        let out = super::protect_setjmp_locals(ir);
        assert!(out.contains("declare i32 @setjmp(ptr) returns_twice"));
        assert!(out.contains("optsize noinline {"));
        assert!(out.contains("store volatile i64 1, ptr %slot"));
        assert!(out.contains("load volatile i64, ptr %slot"));
        assert!(out.contains("load atomic i64, ptr @global monotonic"));
    }
    #[test]
    fn the_buffer_terminator_is_dropped_from_a_wasm_object() {
        let obj = b"\0asm\x01\0\0\0\x01\x01\x00\0".to_vec();
        assert_eq!(super::trim_object(obj.clone()), obj[..obj.len() - 1]);
        // Not a wasm object: untouched.
        assert_eq!(super::trim_object(b"\x7fELF\0".to_vec()), b"\x7fELF\0");
    }

    #[test]
    fn entry_point_is_renamed_for_wasi_libc() {
        let ir = "define i32 @main() {\n  call void @main_helper()\n  ret i32 0\n}\n@llvm.used = [ptr @main]\n";
        let out = super::rename_entry_point(ir);
        assert!(out.contains("define i32 @__main_void() {"), "{out}");
        assert!(out.contains("[ptr @__main_void]"), "{out}");
        assert!(out.contains("call void @main_helper()"), "{out}");
        let with_args = super::rename_entry_point("define i32 @main(i32 %0, ptr %1) {\n}\n");
        assert!(
            with_args.contains("@__main_argc_argv(i32 %0, ptr %1)"),
            "{with_args}"
        );
    }
}

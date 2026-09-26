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
    rename_entry_point(&abi_adapt::adapt_runtime_abi(&ir, &closure_bodies))
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

//! LLVM IR module builder — the top-level `.ll` file.
//!
//! Port of `anvil/src/llvm/module.ts`. Tracks:
//! - external function declarations (deduped; skipped in output if the same
//!   name is also defined in the module, to avoid declare+define conflicts)
//! - string constants (pooled, UTF-8 encoded with a null terminator)
//! - global variables (external, internal, initialized)
//! - function definitions
//!
//! `to_ir()` assembles the pieces into a complete `.ll` file with the target
//! triple header.

use std::collections::HashSet;

use crate::function::LlFunction;
use crate::types::LlvmType;

pub struct LlModule {
    pub target_triple: String,
    declarations: Vec<(String, String)>, // (name, full "declare …" line)
    declared_names: HashSet<String>,
    functions: Vec<LlFunction>,
    globals: Vec<String>,
    string_constants: Vec<String>,
    string_counter: u32,
    /// Extra numbered metadata nodes emitted after `!0 = !{}`. Used by
    /// the buffer alias-scope system to declare per-buffer scopes and
    /// noalias sets so LLVM's LoopVectorizer can prove different buffers
    /// don't alias.
    metadata_lines: Vec<String>,
    /// Module-wide counter for inline cache globals (`perry_ic_N`).
    /// Must be unique across all functions in the module.
    pub ic_counter: u32,
    /// Module-wide counter for buffer alias-scope ids. Each function's
    /// `FnCtx` reads this as its `buffer_alias_base` at creation, then
    /// after the function lowers its body the counter is bumped by the
    /// number of scopes that function allocated. Must be unique across
    /// every function in the module so `!alias.scope !201` references
    /// emitted on loads/stores match the metadata nodes emitted once
    /// at the end of `compile_module` (closes #71).
    pub buffer_alias_counter: u32,
}

impl LlModule {
    pub fn new(target_triple: impl Into<String>) -> Self {
        Self {
            target_triple: target_triple.into(),
            declarations: Vec::new(),
            declared_names: HashSet::new(),
            functions: Vec::new(),
            globals: Vec::new(),
            string_constants: Vec::new(),
            string_counter: 0,
            metadata_lines: Vec::new(),
            ic_counter: 0,
            buffer_alias_counter: 0,
        }
    }

    /// Append a raw metadata definition line (e.g. `!1 = distinct !{!1}`).
    /// Emitted after `!0 = !{}` in the module IR.
    pub fn add_metadata_line(&mut self, line: String) {
        self.metadata_lines.push(line);
    }

    /// Declare an external function (FFI import). Deduped by name — later
    /// calls with the same name are no-ops. If a function with the same name
    /// is later *defined* in this module, the declaration is dropped at
    /// `to_ir` time so LLVM doesn't see both.
    pub fn declare_function(
        &mut self,
        name: &str,
        return_type: LlvmType,
        param_types: &[LlvmType],
    ) {
        if self.declared_names.contains(name) {
            return;
        }
        self.declared_names.insert(name.to_string());
        let param_str = param_types.join(", ");
        // setjmp needs the `returns_twice` attribute to prevent
        // LLVM from promoting alloca slots to SSA registers across
        // the setjmp boundary. Without it, local variables modified
        // between setjmp and longjmp are clobbered when the second
        // return (via longjmp) happens.
        let attrs = if name == "setjmp" || name == "_setjmp" {
            " #0"
        } else {
            ""
        };
        self.declarations.push((
            name.to_string(),
            format!("declare {} @{}({}){}", return_type, name, param_str, attrs),
        ));
    }

    pub fn is_declared(&self, name: &str) -> bool {
        self.declared_names.contains(name)
    }

    /// Define (add) a function. Returns a mutable reference for block
    /// creation.
    pub fn define_function(
        &mut self,
        name: impl Into<String>,
        return_type: LlvmType,
        params: Vec<(LlvmType, String)>,
    ) -> &mut LlFunction {
        let func = LlFunction::new(name, return_type, params);
        self.functions.push(func);
        self.functions.last_mut().unwrap()
    }

    pub fn function_mut(&mut self, idx: usize) -> Option<&mut LlFunction> {
        self.functions.get_mut(idx)
    }

    pub fn add_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = global {} {}", name, ty, init));
    }

    pub fn add_external_global(&mut self, name: &str, ty: LlvmType) {
        self.globals
            .push(format!("@{} = external global {}", name, ty));
    }

    pub fn add_internal_global(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = internal global {} {}", name, ty, init));
    }

    /// Module-private read-only constant. Goes into `.rodata` instead of
    /// `.data` and the linker may merge identical copies across compilation
    /// units. Used by the ExternFuncRef-as-value path to emit static
    /// `ClosureHeader` records pointing at `__perry_wrap_extern_*` thunks
    /// — those are pure data and never mutated at runtime.
    pub fn add_internal_constant(&mut self, name: &str, ty: LlvmType, init: &str) {
        self.globals
            .push(format!("@{} = internal constant {} {}", name, ty, init));
    }

    /// Push a fully-formed `@<name> = ...` line into the module's globals
    /// list. Used for constants whose type is not in the `LlvmType` enum
    /// (e.g. `[N x i32]` flat constant arrays for issue #50's folded
    /// module-level 2D int arrays).
    pub fn add_raw_global(&mut self, line: String) {
        self.globals.push(line);
    }

    /// Add a string constant with a caller-controlled name. Used by the
    /// `StringPool` so that emission order matches the pool's interned
    /// indices and the bytes globals can be referenced by name from
    /// `__perry_init_strings`.
    ///
    /// `escaped_lit` is the full LLVM IR literal *including* the surrounding
    /// `c"…"` and the trailing `\00`. `total_bytes` is the array length
    /// (= byte_len + 1 for the null terminator).
    pub fn add_named_string_constant(&mut self, name: &str, total_bytes: usize, escaped_lit: &str) {
        self.string_constants.push(format!(
            "@{} = private unnamed_addr constant [{} x i8] {}",
            name, total_bytes, escaped_lit
        ));
    }

    /// Add a UTF-8 string constant to the module's constant pool. Returns
    /// `(global_name, byte_length)` — the byte length is what Perry passes as
    /// the `len` argument to `js_string_from_bytes`.
    pub fn add_string_constant(&mut self, value: &str) -> (String, usize) {
        let name = format!(".str.{}", self.string_counter);
        self.string_counter += 1;

        let bytes = value.as_bytes();
        let len = bytes.len();
        let array_type = format!("[{} x i8]", len + 1);

        // Encode as an LLVM IR C-style string: printable ASCII pass through,
        // everything else becomes `\xx` hex escapes. Then append `\00` for
        // the C null terminator.
        let mut lit = String::with_capacity(len + 8);
        lit.push_str("c\"");
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");

        self.string_constants.push(format!(
            "@{} = private unnamed_addr constant {} {}",
            name, array_type, lit
        ));
        (name, len)
    }

    /// Serialize the module to a complete `.ll` file.
    pub fn to_ir(&self) -> String {
        let mut ir = String::new();
        ir.push_str("; Generated by perry-codegen\n");
        ir.push_str(&format!("target triple = \"{}\"\n\n", self.target_triple));

        for sc in &self.string_constants {
            ir.push_str(sc);
            ir.push('\n');
        }
        ir.push('\n');

        for g in &self.globals {
            ir.push_str(g);
            ir.push('\n');
        }
        ir.push('\n');

        // Skip any `declare` whose name is also `define`d in this module —
        // LLVM rejects declare+define for the same symbol.
        let defined: HashSet<&str> = self.functions.iter().map(|f| f.name.as_str()).collect();
        for (name, decl) in &self.declarations {
            if defined.contains(name.as_str()) {
                continue;
            }
            ir.push_str(decl);
            ir.push('\n');
        }
        ir.push('\n');

        for func in &self.functions {
            ir.push_str(&func.to_ir());
            ir.push('\n');
        }

        // Attribute group for setjmp's `returns_twice` marker.
        // Only emit if setjmp was actually declared in this module.
        if self.declared_names.contains("setjmp") {
            ir.push_str("\nattributes #0 = { returns_twice }\n");
            // Functions that contain a `try` statement are marked with `#1`.
            // `optnone` forces LLVM to skip mem2reg/SROA inside the function,
            // so allocas aren't promoted to SSA registers across the setjmp
            // call — otherwise mutations in the try body are invisible to
            // the catch block after longjmp. Pairs with `noinline` so the
            // constraint isn't lost via inlining into a caller.
            ir.push_str("attributes #1 = { noinline optnone }\n");
        }

        // Issue #52: `!0 = !{}` metadata node referenced by
        // `load_invariant` (via `!invariant.load !0`). LLVM's GVN + LICM
        // hoist loads tagged with `!invariant.load` out of their
        // enclosing loops when the loop body can't write to the same
        // address; without this, the per-access Buffer / Array length
        // reload stays pinned inside every bounds check even when the
        // buffer is loop-invariant.
        ir.push_str("\n!0 = !{}\n");
        for ml in &self.metadata_lines {
            ir.push_str(ml);
            ir.push('\n');
        }

        ir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DOUBLE, I32, I64, PTR, VOID};

    #[test]
    fn hello_world_ir_is_well_formed() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("js_console_log_number", VOID, &[DOUBLE]);
        let (_sname, _slen) = m.add_string_constant("hello");

        let f = m.define_function("main", I32, vec![]);
        let entry = f.create_block("entry");
        entry.call_void("js_console_log_number", &[(DOUBLE, "42.0")]);
        entry.ret(I32, "0");

        let ir = m.to_ir();
        assert!(ir.contains("target triple = \"arm64-apple-macosx15.0.0\""));
        assert!(ir.contains("declare void @js_console_log_number(double)"));
        assert!(ir.contains("define i32 @main()"));
        assert!(ir.contains("call void @js_console_log_number(double 42.0)"));
        assert!(ir.contains("ret i32 0"));
    }

    #[test]
    fn declare_is_dropped_when_also_defined() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        m.declare_function("main", I32, &[]);
        let f = m.define_function("main", I32, vec![]);
        f.create_block("entry").ret(I32, "0");
        let ir = m.to_ir();
        assert!(!ir.contains("declare i32 @main"));
        assert!(ir.contains("define i32 @main"));
    }

    #[test]
    fn string_constant_escapes_nonprintable() {
        let mut m = LlModule::new("arm64-apple-macosx15.0.0");
        let (name, len) = m.add_string_constant("a\nb");
        assert_eq!(name, ".str.0");
        assert_eq!(len, 3);
        let ir = m.to_ir();
        // "a" then \0A then "b" then \00
        assert!(ir.contains("c\"a\\0Ab\\00\""), "got: {}", ir);
    }

    #[test]
    fn gep_unused_helper_imports_compile() {
        // Smoke test that PTR, I64 are re-exported and compile alongside.
        let _ = (PTR, I64);
    }
}

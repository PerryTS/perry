//! String literal pool with module-init hoisting + interning.
//!
//! ## Strategy
//!
//! Every string literal in the source program is allocated **once** at module
//! startup, not at each use site. Use sites become a single `load`
//! instruction. Identical literals share storage via interning, so
//! `console.log("hi")` written 1000 times produces 1000 loads but only one
//! `js_string_from_bytes` call at init time.
//!
//! ### What gets emitted per literal
//!
//! For each unique literal `"<value>"`, we emit two LLVM globals:
//!
//! ```llvm
//! @.str.<idx>.bytes  = private unnamed_addr constant [<len+1> x i8] c"<value>\00"
//! @.str.<idx>.handle = internal global double 0.0
//! ```
//!
//! The `bytes` global lives in `.rodata` — it's static, immutable, and never
//! touched by the GC. The `handle` global is mutable and holds the
//! NaN-boxed string pointer that the runtime allocates at init time.
//!
//! ### Module init function
//!
//! The codegen also emits a `void __perry_init_strings()` function that runs
//! once before user code and:
//!
//! 1. Calls `js_string_from_bytes(@.str.<idx>.bytes, <len>)` to allocate a
//!    `StringHeader` on the GC heap with the literal's bytes copied in.
//! 2. Calls `js_nanbox_string(handle)` to wrap the raw pointer with the
//!    `STRING_TAG`.
//! 3. Stores the NaN-boxed double into `@.str.<idx>.handle`.
//! 4. Calls `js_gc_register_global_root(&@.str.<idx>.handle)` so the runtime
//!    treats the global as a permanent root and never collects the string.
//!
//! Step 4 is the load-bearing one: without it, the next GC cycle would walk
//! its `MALLOC_OBJECTS` Vec, find the string unreferenced from the stack,
//! and free it — leaving every use site loading a dangling pointer.
//! `js_gc_register_global_root` is defined in
//! `crates/perry-runtime/src/gc.rs:233` and pushes the address into a
//! `GLOBAL_ROOTS` Vec that the mark phase scans alongside the stack.
//!
//! ### Use site
//!
//! `Expr::String(s)` lowers to:
//!
//! ```llvm
//! %r = load double, ptr @.str.<idx>.handle
//! ```
//!
//! That's the entire codegen for a string literal at the use site. One
//! instruction. No call, no allocation, no GC pressure. The literal cost
//! is paid exactly once at process startup, no matter how often the literal
//! appears in hot code.
//!
//! ### Why a pool instead of per-use-site allocation
//!
//! A naive approach would re-create every string literal at every use
//! site: stack-allocate the bytes, call `js_string_from_bytes`, NaN-box
//! the result. That's ~5 IR instructions per use, plus a heap allocation.
//! For a literal used 1000 times in a loop, that's 1000 allocations and
//! 1000 short-lived StringHeaders the GC has to sweep.
//! The pool approach: 1 allocation, 1 root registration, 1000 loads.

use std::collections::HashMap;

pub struct StringPool {
    /// Module symbol prefix used in every emitted global name. Set at
    /// construction time so the pool's `bytes_global`/`handle_global`
    /// names match what `emit_string_pool` generates and the codegen
    /// use sites can reference them directly.
    module_prefix: String,
    /// `value → interned index`. Identical literals share an entry.
    interned: HashMap<String, u32>,
    /// Ordered list of unique entries; the index in this Vec is the
    /// interned index referenced by `interned`.
    entries: Vec<StringEntry>,
    /// #5247: source-location context for the dynamic call-dispatch throw
    /// path. Set once per module after construction (only when the CLI
    /// `--debug-symbols` flag is on). `None` in the default build so codegen
    /// emits no per-call `js_set_call_location` overhead. When `Some`,
    /// carries `(module_file_path, module_source)` so the call-lowering site
    /// can resolve a `Call.byte_offset` to a `file:line`.
    debug_location_ctx: Option<(String, String)>,
    /// #5247 (CJS-wrap coordinate skew): newlines the wrapper prefix added
    /// before the original body in `debug_location_ctx`'s (wrapped) source.
    /// `call_location_for` subtracts this from the wrapped line number to
    /// recover the original-source line. `0` for non-wrapped modules.
    debug_source_line_offset: u32,
    /// #5247: the byte offset of the `Expr::Call` currently being lowered,
    /// recorded by the call dispatcher and consumed at the dynamic
    /// method-dispatch emission site (after the call's arguments — which may
    /// themselves be nested calls that overwrite this — have been lowered) so
    /// the `js_set_call_location` is emitted with the *outer* call's offset
    /// immediately before the throwing dispatch. `0` = none.
    pending_call_offset: std::cell::Cell<u32>,
}

pub struct StringEntry {
    pub idx: u32,
    pub value: String,
    pub byte_len: usize,
    /// LLVM IR escaped form, e.g. `c"hello\00"`. Already includes the
    /// trailing null terminator and the surrounding `c"…"`.
    pub escaped_ir: String,
    /// Symbol name of the `.rodata` byte array (`.str.N.bytes`).
    pub bytes_global: String,
    /// Symbol name of the mutable handle global (`.str.N.handle`).
    pub handle_global: String,
    /// true = bytes contain WTF-8 lone surrogates; use js_string_from_wtf8_bytes at init.
    pub is_wtf8: bool,
}

impl StringPool {
    pub fn new() -> Self {
        Self::with_prefix(String::new())
    }

    /// Construct a pool whose emitted global names will be prefixed with
    /// `module_prefix`. The codegen passes the per-module prefix so that
    /// multiple modules in the same link can each have their own pool
    /// without colliding on `.str.0.handle` etc.
    pub fn with_prefix(module_prefix: String) -> Self {
        Self {
            module_prefix,
            interned: HashMap::new(),
            entries: Vec::new(),
            debug_location_ctx: None,
            debug_source_line_offset: 0,
            pending_call_offset: std::cell::Cell::new(0),
        }
    }

    pub fn module_prefix(&self) -> &str {
        &self.module_prefix
    }

    /// #5247: install the per-module source-location context (file path +
    /// source text) consulted by the dynamic call-dispatch lowering when the
    /// `--debug-symbols` flag is on. No-op otherwise (`ctx` is `None`).
    pub fn set_debug_location_ctx(&mut self, ctx: Option<(String, String)>) {
        self.debug_location_ctx = ctx;
    }

    /// #5247 (CJS-wrap coordinate skew): set the wrapper-prefix line count that
    /// `call_location_for` subtracts from the wrapped line number. `0` (the
    /// default) leaves resolution unchanged.
    pub fn set_debug_source_line_offset(&mut self, offset: u32) {
        self.debug_source_line_offset = offset;
    }

    /// #5247: true iff source-location tracking is active for this module
    /// (i.e. `--debug-symbols` installed a debug-location context). Lets the
    /// dispatch emission site skip all location work in the default build.
    pub fn debug_locations_enabled(&self) -> bool {
        self.debug_location_ctx.is_some()
    }

    /// #5247: record the byte offset of the call currently being lowered.
    pub fn set_pending_call_offset(&self, byte_offset: u32) {
        self.pending_call_offset.set(byte_offset);
    }

    /// #5247: the byte offset recorded by the most recent
    /// [`set_pending_call_offset`]. `0` when none.
    pub fn pending_call_offset(&self) -> u32 {
        self.pending_call_offset.get()
    }

    /// #5247: resolve a `Call`'s source byte offset to `(file_path, line)`,
    /// where `line` is 1-based. Returns `None` when no debug-location context
    /// is installed (default build), the offset is `0` (synthesized call), or
    /// the offset is out of range. SWC's `BytePos` is 1-based, matching the
    /// `lower` crate's `current_module_source_slice`, so subtract 1.
    pub fn call_location_for(&self, byte_offset: u32) -> Option<(&str, u32)> {
        if byte_offset == 0 {
            return None;
        }
        let (file, src) = self.debug_location_ctx.as_ref()?;
        let offset = (byte_offset.saturating_sub(1)) as usize;
        if offset > src.len() {
            return None;
        }
        // 1-based line = 1 + count of newlines before the offset.
        let wrapped_line = 1 + src.as_bytes()[..offset]
            .iter()
            .filter(|&&b| b == b'\n')
            .count() as u32;
        // #5247 (CJS-wrap coordinate skew): `src` is the WRAPPED source for a
        // CommonJS module, so deduct the wrapper-prefix line count to recover
        // the original-source line. A wrapped line at or inside the preamble
        // (`wrapped_line <= offset`) has no original counterpart → no location.
        let line = if self.debug_source_line_offset == 0 {
            wrapped_line
        } else if wrapped_line > self.debug_source_line_offset {
            wrapped_line - self.debug_source_line_offset
        } else {
            return None;
        };
        Some((file.as_str(), line))
    }

    /// Intern a string literal. Returns the interned index, stable for the
    /// life of the pool. Identical strings collapse to the same index.
    pub fn intern(&mut self, value: &str) -> u32 {
        if let Some(&idx) = self.interned.get(value) {
            return idx;
        }
        let idx = self.entries.len() as u32;
        let byte_len = value.len(); // UTF-8 byte length, what js_string_from_bytes expects
        let escaped_ir = escape_for_llvm_ir(value.as_bytes());
        let bytes_global = if self.module_prefix.is_empty() {
            format!(".str.{}.bytes", idx)
        } else {
            format!("{}_.str.{}.bytes", self.module_prefix, idx)
        };
        let handle_global = if self.module_prefix.is_empty() {
            format!(".str.{}.handle", idx)
        } else {
            format!("{}_.str.{}.handle", self.module_prefix, idx)
        };
        let entry = StringEntry {
            idx,
            value: value.to_string(),
            byte_len,
            escaped_ir,
            bytes_global,
            handle_global,
            is_wtf8: false,
        };
        self.entries.push(entry);
        self.interned.insert(value.to_string(), idx);
        idx
    }

    /// Intern a WTF-8 byte sequence (may contain lone surrogates).
    /// Uses a separate key-space from normal strings (prefixed "wtf8:").
    pub fn intern_wtf8(&mut self, bytes: &[u8]) -> u32 {
        let key = format!("wtf8:{}", bytes.escape_ascii());
        if let Some(&idx) = self.interned.get(&key) {
            return idx;
        }
        let idx = self.entries.len() as u32;
        let byte_len = bytes.len();
        let escaped_ir = escape_for_llvm_ir(bytes);
        let bytes_global = if self.module_prefix.is_empty() {
            format!(".str.{}.bytes", idx)
        } else {
            format!("{}_.str.{}.bytes", self.module_prefix, idx)
        };
        let handle_global = if self.module_prefix.is_empty() {
            format!(".str.{}.handle", idx)
        } else {
            format!("{}_.str.{}.handle", self.module_prefix, idx)
        };
        let entry = StringEntry {
            idx,
            value: key.clone(),
            byte_len,
            escaped_ir,
            bytes_global,
            handle_global,
            is_wtf8: true,
        };
        self.entries.push(entry);
        self.interned.insert(key, idx);
        idx
    }

    pub fn entry(&self, idx: u32) -> &StringEntry {
        &self.entries[idx as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &StringEntry> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for StringPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode a UTF-8 byte slice as an LLVM IR string literal: printable ASCII
/// passes through, everything else (including `"` and `\`) becomes `\xx`
/// hex escapes. The result includes the surrounding `c"…"` and the trailing
/// `\00` null terminator.
fn escape_for_llvm_ir(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 8);
    s.push_str("c\"");
    for &b in bytes {
        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
            s.push(b as char);
        } else {
            s.push('\\');
            s.push_str(&format!("{:02X}", b));
        }
    }
    s.push_str("\\00\"");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_dedupes_identical_strings() {
        let mut pool = StringPool::new();
        let a = pool.intern("hello");
        let b = pool.intern("hello");
        let c = pool.intern("world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn entries_have_correct_byte_lengths() {
        let mut pool = StringPool::new();
        let idx = pool.intern("hello world");
        let e = pool.entry(idx);
        assert_eq!(e.byte_len, 11);
        assert_eq!(e.bytes_global, ".str.0.bytes");
        assert_eq!(e.handle_global, ".str.0.handle");
    }

    #[test]
    fn escape_handles_quotes_backslashes_newlines() {
        let mut pool = StringPool::new();
        let idx = pool.intern("a\"b\\c\nd");
        let e = pool.entry(idx);
        // " (0x22) → \22, \ (0x5C) → \5C, \n (0x0A) → \0A, then \00 terminator
        assert_eq!(e.escaped_ir, "c\"a\\22b\\5Cc\\0Ad\\00\"");
        assert_eq!(e.byte_len, 7);
    }

    #[test]
    fn empty_string_works() {
        let mut pool = StringPool::new();
        let idx = pool.intern("");
        assert_eq!(idx, 0);
        let e = pool.entry(idx);
        assert_eq!(e.byte_len, 0);
        assert_eq!(e.escaped_ir, "c\"\\00\"");
    }

    #[test]
    fn utf8_multibyte_byte_length_is_byte_count_not_char_count() {
        let mut pool = StringPool::new();
        // "héllo" — é is 2 bytes (0xC3 0xA9). Total: 6 bytes, 5 chars.
        let idx = pool.intern("héllo");
        let e = pool.entry(idx);
        assert_eq!(e.byte_len, 6);
    }

    // ───────────────────── #5247 byte→line mapping ─────────────────────
    // `call_location_for` takes an SWC `BytePos` (1-based), so byte offset N
    // refers to source index N-1. Line is 1-based (1 + newlines before it).

    fn pool_with_src(src: &str) -> StringPool {
        let mut p = StringPool::new();
        p.set_debug_location_ctx(Some(("foo.ts".to_string(), src.to_string())));
        p
    }

    #[test]
    fn call_location_none_without_debug_context() {
        // Default build: no context installed → never resolves a location.
        let p = StringPool::new();
        assert_eq!(p.call_location_for(5), None);
    }

    #[test]
    fn call_location_zero_offset_is_none() {
        // 0 sentinel (synthesized call) → no location.
        let p = pool_with_src("a\nb\nc");
        assert_eq!(p.call_location_for(0), None);
    }

    #[test]
    fn call_location_first_line() {
        // Offsets within the first line (before any '\n') → line 1.
        let p = pool_with_src("foo();\nbar();\n");
        // BytePos 1 = source index 0 ('f'); BytePos 6 = index 5 (')').
        assert_eq!(p.call_location_for(1), Some(("foo.ts", 1)));
        assert_eq!(p.call_location_for(6), Some(("foo.ts", 1)));
    }

    #[test]
    fn call_location_line_boundaries() {
        // "foo();\nbar();\nbaz();\n"
        //  index:0..5  '\n'=6  7..12 '\n'=13  14..19 '\n'=20
        let src = "foo();\nbar();\nbaz();\n";
        let p = pool_with_src(src);
        // BytePos for the '\n' itself (index 6 → BytePos 7) still counts as
        // line 1 (no newline strictly *before* it).
        assert_eq!(p.call_location_for(7), Some(("foo.ts", 1)));
        // First char of line 2 ('b' at index 7 → BytePos 8): one '\n' before.
        assert_eq!(p.call_location_for(8), Some(("foo.ts", 2)));
        // First char of line 3 ('b' at index 14 → BytePos 15): two '\n' before.
        assert_eq!(p.call_location_for(15), Some(("foo.ts", 3)));
    }

    #[test]
    fn call_location_last_line_and_out_of_range() {
        let src = "x\ny\nz"; // 5 bytes, 3 lines, no trailing newline
        let p = pool_with_src(src);
        // 'z' at index 4 → BytePos 5: two '\n' before → line 3.
        assert_eq!(p.call_location_for(5), Some(("foo.ts", 3)));
        // BytePos == len+1 (index == len): clamped, still line 3 (EOF after z).
        assert_eq!(p.call_location_for(6), Some(("foo.ts", 3)));
        // Far out of range → None.
        assert_eq!(p.call_location_for(100), None);
    }

    #[test]
    fn call_location_utf8_safe() {
        // Multi-byte chars before the call must not panic or miscount lines —
        // we count raw bytes, and the offsets are byte offsets, so slicing on
        // a byte boundary the compiler produced is always valid.
        // "café();\nx();" — "café();" is 8 bytes (é = 2), '\n' at index 8.
        let src = "café();\nx();";
        let p = pool_with_src(src);
        // 'x' is at byte index 9 (after "café();\n" = 9 bytes) → BytePos 10.
        assert_eq!(p.call_location_for(10), Some(("foo.ts", 2)));
        // A position inside line 1 → line 1.
        assert_eq!(p.call_location_for(2), Some(("foo.ts", 1)));
    }

    // ───────────── #5247 CJS-wrap coordinate skew correction ─────────────
    // For a CJS-wrapped module `src` is the WRAPPED text and offsets are in
    // wrapped coords. `set_debug_source_line_offset(N)` makes `call_location_for`
    // report `wrapped_line - N` so the location is in original-source coords.

    #[test]
    fn cjs_wrap_offset_subtracts_prefix_lines() {
        // Wrapped: 3 preamble lines, then the original body's two lines.
        //   line1: import _req_0 ...
        //   line2: const _cjs = (function() {
        //   line3:   /* preamble */
        //   line4:   foo();   <- original body line 1
        //   line5:   bar();   <- original body line 2
        let src = "import _req_0;\nconst _cjs = (function() {\n  // preamble\n  foo();\n  bar();\n";
        let mut p = pool_with_src(src);
        p.set_debug_source_line_offset(3);
        // 'f' of foo() is on wrapped line 4 → original line 1.
        let off_foo = (src.find("foo();").unwrap() + 1) as u32;
        assert_eq!(p.call_location_for(off_foo), Some(("foo.ts", 1)));
        // 'b' of bar() is on wrapped line 5 → original line 2.
        let off_bar = (src.find("bar();").unwrap() + 1) as u32;
        assert_eq!(p.call_location_for(off_bar), Some(("foo.ts", 2)));
    }

    #[test]
    fn cjs_wrap_offset_inside_preamble_is_none() {
        // An offset whose wrapped line is at/inside the prefix has no original
        // counterpart → no location (rather than a bogus negative/zero line).
        let src = "import _req_0;\nconst _cjs = (function() {\n  // preamble\n  foo();\n";
        let mut p = pool_with_src(src);
        p.set_debug_source_line_offset(3);
        // Offset on wrapped line 1 (the injected import) → None.
        assert_eq!(p.call_location_for(3), None);
        // Offset on wrapped line 3 (preamble, == prefix count) → None.
        let off_preamble = (src.find("// preamble").unwrap() + 1) as u32;
        assert_eq!(p.call_location_for(off_preamble), None);
    }

    #[test]
    fn zero_offset_leaves_lines_unchanged() {
        // Non-wrapped module (offset 0): line resolution is the raw wrapped line.
        let src = "foo();\nbar();\n";
        let mut p = pool_with_src(src);
        p.set_debug_source_line_offset(0);
        let off_bar = (src.find("bar();").unwrap() + 1) as u32;
        assert_eq!(p.call_location_for(off_bar), Some(("foo.ts", 2)));
    }
}

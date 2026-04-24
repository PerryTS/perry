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
        }
    }

    pub fn module_prefix(&self) -> &str {
        &self.module_prefix
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
}

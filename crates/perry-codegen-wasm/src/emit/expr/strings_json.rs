//! String methods (split/fromCharCode/at/match/replace/coerce) and JSON parse/stringify.
//!
//! Mechanically extracted from emit/expr.rs (#1102 follow-up split).
//! See `mod.rs` for the dispatcher that calls each `try_emit_expr_*`.

use super::*;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_strings_json(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            Expr::StringSplit(string, delim) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, delim);
                self.emit_memcall(func, "string_split", 2);
            }
            Expr::StringFromCharCode(code) => {
                // Bridge name is the key in __memDispatch (wasm_runtime.js) — keep
                // camelCase even though Rust prefers snake_case; no dispatch entry
                // means mem_call silently falls through to __classDispatch.
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_fromCharCode", 1);
            }
            Expr::StringFromCodePoint(code) => {
                // WASM stub: same as fromCharCode for now (BMP-only).
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_fromCharCode", 1);
            }
            Expr::StringAt { string, index } => {
                // WASM stub: alias to char_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_at", 2);
            }
            Expr::StringCodePointAt { string, index } => {
                // WASM stub: alias to char_code_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_code_at", 2);
            }
            Expr::StringMatch { string, regex } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, regex);
                self.emit_memcall(func, "string_match", 2);
            }
            Expr::StringReplace {
                string,
                pattern,
                replacement,
            } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, pattern);
                self.emit_store_arg(func, 2, replacement);
                self.emit_memcall(func, "string_replace", 3);
            }
            Expr::StringCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "jsvalue_to_string", 1);
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_parse", 1);
            }
            Expr::JsonStringify(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_stringify", 1);
            }

            _ => return false,
        }
        true
    }
}

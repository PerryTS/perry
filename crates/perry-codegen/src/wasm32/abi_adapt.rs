//! Route every runtime call through the runtime's real wasm32 signature.
//!
//! Codegen carries pointers as `i64` (NaN-box payloads, handles, raw heap
//! addresses), declares most runtime helpers that way, and lets each `call`
//! carry its own function type. On LP64 targets that is ABI-identical to the
//! runtime's `*mut T`/`usize` parameters. On wasm32 it is not: a pointer is
//! `i32`, and a call whose type differs from its callee's is a link-time
//! signature mismatch that traps (`scripts/runtime_abi_check.py` counts the
//! sites — over a thousand).
//!
//! Rather than retype every one of those sites (and risk LP64 output), this
//! pass rewrites the finished module text for wasm32 only:
//!
//! * each `declare` of a runtime symbol becomes the runtime's real signature,
//!   read from `runtime_abi.tsv` — generated from the runtime's source by
//!   `runtime_abi_check.py --emit-wasm-abi` and freshness-checked in CI;
//! * each `call`/`invoke` of such a symbol whose own type differs is pointed
//!   at a small `internal alwaysinline` adapter with exactly the call's type,
//!   which converts every argument to the real type (`trunc`/`inttoptr` for
//!   i64 → pointer-width, zero/sign extension per the runtime type's
//!   signedness on the way back) and calls the real function.
//!
//! After inlining the adapters cost nothing. Calls that already match are
//! left untouched, so the pass is the identity on a module with no
//! mismatches.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::OnceLock;

/// One ABI slot of a runtime signature, as `runtime_abi.tsv` spells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    F64,
    F32,
    I64,
    I32 {
        signed: bool,
    },
    I16 {
        signed: bool,
    },
    I8 {
        signed: bool,
    },
    Bool,
    Ptr,
    /// `usize`/`isize`: an `i32` on wasm32.
    Size {
        signed: bool,
    },
    Void,
}

impl Tok {
    fn parse(s: &str) -> Option<Tok> {
        Some(match s {
            "f64" => Tok::F64,
            "f32" => Tok::F32,
            "i64" => Tok::I64,
            "i32s" => Tok::I32 { signed: true },
            "i32u" => Tok::I32 { signed: false },
            "i16s" => Tok::I16 { signed: true },
            "i16u" => Tok::I16 { signed: false },
            "i8s" => Tok::I8 { signed: true },
            "i8u" => Tok::I8 { signed: false },
            "bool" => Tok::Bool,
            "ptr" => Tok::Ptr,
            "isize" => Tok::Size { signed: true },
            "usize" => Tok::Size { signed: false },
            "void" => Tok::Void,
            _ => return None,
        })
    }

    fn ir(self) -> &'static str {
        match self {
            Tok::F64 => "double",
            Tok::F32 => "float",
            Tok::I64 => "i64",
            Tok::I32 { .. } | Tok::Size { .. } => "i32",
            Tok::I16 { .. } => "i16",
            Tok::I8 { .. } => "i8",
            Tok::Bool => "i1",
            Tok::Ptr => "ptr",
            Tok::Void => "void",
        }
    }

    /// Whether widening this value to a larger integer sign-extends.
    fn signed(self) -> bool {
        match self {
            Tok::I32 { signed }
            | Tok::I16 { signed }
            | Tok::I8 { signed }
            | Tok::Size { signed } => signed,
            _ => false,
        }
    }
}

struct Sig {
    ret: Tok,
    params: Vec<Tok>,
}

impl Sig {
    fn ir_shape(&self) -> (&'static str, Vec<&'static str>) {
        (self.ret.ir(), self.params.iter().map(|t| t.ir()).collect())
    }
}

const TABLE_SRC: &str = include_str!("runtime_abi.tsv");

fn table() -> &'static HashMap<&'static str, Sig> {
    static TABLE: OnceLock<HashMap<&'static str, Sig>> = OnceLock::new();
    TABLE.get_or_init(|| parse_table(TABLE_SRC))
}

fn parse_table(src: &'static str) -> HashMap<&'static str, Sig> {
    let mut out = HashMap::new();
    for line in src.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(name), Some(ret), params) = (cols.next(), cols.next(), cols.next()) else {
            panic!("runtime_abi.tsv: malformed line {line:?}");
        };
        let ret =
            Tok::parse(ret).unwrap_or_else(|| panic!("runtime_abi.tsv: bad return in {line:?}"));
        let params = params
            .unwrap_or("")
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|p| {
                Tok::parse(p).unwrap_or_else(|| panic!("runtime_abi.tsv: bad param in {line:?}"))
            })
            .collect();
        out.insert(name, Sig { ret, params });
    }
    out
}

/// A parsed call/invoke line: everything but the callee is kept verbatim.
struct CallSite<'a> {
    /// The line up to and including the callee's `@`.
    head: &'a str,
    name: &'a str,
    ret: &'a str,
    /// `(type, value)` per argument.
    args: Vec<(&'a str, &'a str)>,
    /// Text from `(` (inclusive) to end of line.
    tail: &'a str,
}

/// `[%r = ][tail |musttail |notail ](call|invoke) [cc ]<ret> @name(<args>)…`
fn parse_call(line: &str) -> Option<CallSite<'_>> {
    let trimmed = line.trim_start();
    let mut rest = trimmed;
    if rest.starts_with('%') {
        let eq = rest.find(" = ")?;
        rest = &rest[eq + 3..];
    }
    for prefix in ["tail ", "musttail ", "notail "] {
        if let Some(r) = rest.strip_prefix(prefix) {
            rest = r;
            break;
        }
    }
    rest = rest
        .strip_prefix("call ")
        .or_else(|| rest.strip_prefix("invoke "))?;
    let at = rest.find(" @")?;
    let before = &rest[..at];
    // `before` is `[cc ]<ret>`; the return type is its last token.
    let ret = before.rsplit(' ').next()?;
    if ret.is_empty() || ret.contains('(') {
        return None; // indirect call or a type we do not adapt
    }
    let after = &rest[at + 2..];
    let paren = after.find('(')?;
    let name = &after[..paren];
    if name.is_empty() || name.starts_with('"') {
        return None;
    }
    let args_start = paren + 1;
    let close = matching_paren(after, paren)?;
    let args_text = &after[args_start..close];
    let mut args = Vec::new();
    for a in split_top(args_text) {
        let a = a.trim();
        let sp = a.find(' ')?;
        args.push((&a[..sp], a[sp + 1..].trim()));
    }
    let head_len = line.len() - after.len();
    Some(CallSite {
        head: &line[..head_len],
        name,
        ret,
        args,
        tail: &after[paren..],
    })
}

fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    for (i, ch) in s.char_indices().skip_while(|(i, _)| *i < open) {
        match ch {
            '"' => in_str = !in_str,
            '(' | '[' | '{' | '<' if !in_str => depth += 1,
            ')' | ']' | '}' | '>' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start, mut in_str) = (Vec::new(), 0i32, 0usize, false);
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '(' | '[' | '{' | '<' if !in_str => depth += 1,
            ')' | ']' | '}' | '>' if !in_str => depth -= 1,
            ',' if depth == 0 && !in_str => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if !s[start..].trim().is_empty() {
        out.push(&s[start..]);
    }
    out
}

/// `declare … <ret> @name(<params>)<suffix>` → (name, suffix after `)`).
fn parse_declare(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("declare ")?;
    let at = rest.find(" @")?;
    let after = &rest[at + 2..];
    let paren = after.find('(')?;
    let close = matching_paren(after, paren)?;
    Some((&after[..paren], &after[close + 1..]))
}

fn int_bits(ty: &str) -> Option<u32> {
    ty.strip_prefix('i').and_then(|b| b.parse().ok())
}

fn zero_of(ty: &str) -> &'static str {
    match ty {
        "double" | "float" => "0.0",
        "ptr" => "null",
        _ => "0",
    }
}

fn fresh(body: &mut String, n: &mut u32, inst: String) -> String {
    *n += 1;
    let r = format!("%c{n}");
    let _ = writeln!(body, "  {r} = {inst}");
    r
}

/// Emit instructions converting `val: from` to `to`, returning the new value.
/// `signed` decides sign- vs zero-extension when an integer widens.
fn convert(
    body: &mut String,
    n: &mut u32,
    val: &str,
    from: &str,
    to: &str,
    signed: bool,
) -> String {
    if from == to {
        return val.to_string();
    }
    match (from, to) {
        ("double", "float") => fresh(body, n, format!("fptrunc double {val} to float")),
        ("float", "double") => fresh(body, n, format!("fpext float {val} to double")),
        ("ptr", t) if int_bits(t).is_some() => fresh(body, n, format!("ptrtoint ptr {val} to {t}")),
        (f, "ptr") if int_bits(f).is_some() => fresh(body, n, format!("inttoptr {f} {val} to ptr")),
        (f, t) if int_bits(f).is_some() && int_bits(t).is_some() => {
            let (fb, tb) = (int_bits(f).unwrap(), int_bits(t).unwrap());
            let op = if fb > tb {
                "trunc"
            } else if signed && fb > 1 {
                "sext"
            } else {
                "zext"
            };
            fresh(body, n, format!("{op} {f} {val} to {t}"))
        }
        // Mixed float/integer/pointer (the "class" mismatches the checker
        // reports on every target): move the bits through i64, which is what
        // those call sites assume when they box or unbox by hand.
        ("double", t) => {
            let bits = fresh(body, n, format!("bitcast double {val} to i64"));
            convert(body, n, &bits, "i64", t, signed)
        }
        (f, "double") => {
            let bits = convert(body, n, val, f, "i64", signed);
            fresh(body, n, format!("bitcast i64 {bits} to double"))
        }
        ("float", t) => {
            let bits = fresh(body, n, format!("bitcast float {val} to i32"));
            convert(body, n, &bits, "i32", t, signed)
        }
        (f, "float") => {
            let bits = convert(body, n, val, f, "i32", signed);
            fresh(body, n, format!("bitcast i32 {bits} to float"))
        }
        _ => val.to_string(),
    }
}

fn adapter(
    name: &str,
    sig: &Sig,
    call_ret: &str,
    call_args: &[&str],
    adapter_name: &str,
) -> String {
    let (real_ret, real_params) = sig.ir_shape();
    let mut body = String::new();
    let mut n = 0u32;
    let mut real_args = Vec::new();
    for (i, (real_ty, tok)) in real_params.iter().zip(&sig.params).enumerate() {
        match call_args.get(i) {
            Some(from) => {
                let v = convert(
                    &mut body,
                    &mut n,
                    &format!("%a{i}"),
                    from,
                    real_ty,
                    tok.signed(),
                );
                real_args.push(format!("{real_ty} {v}"));
            }
            // A call site passing fewer arguments than the runtime reads
            // (an arity mismatch on every target): pass a defined zero.
            None => real_args.push(format!("{real_ty} {}", zero_of(real_ty))),
        }
    }
    let params: Vec<String> = call_args
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{t} %a{i}"))
        .collect();
    let call = format!("call {real_ret} @{name}({})", real_args.join(", "));
    if real_ret == "void" {
        let _ = writeln!(body, "  {call}");
    } else {
        let _ = writeln!(body, "  %r = {call}");
    }
    if call_ret == "void" {
        let _ = writeln!(body, "  ret void");
    } else if real_ret == "void" {
        let _ = writeln!(body, "  ret {call_ret} {}", zero_of(call_ret));
    } else {
        let v = convert(
            &mut body,
            &mut n,
            "%r",
            real_ret,
            call_ret,
            sig.ret.signed(),
        );
        let _ = writeln!(body, "  ret {call_ret} {v}");
    }
    format!(
        "define internal {call_ret} @{adapter_name}({}) alwaysinline {{\nentry:\n{body}}}\n",
        params.join(", ")
    )
}

/// Rewrite one module's IR text so every call to a runtime symbol matches the
/// runtime's real wasm32 signature. The identity on a module with nothing to
/// adapt.
pub(crate) fn adapt_runtime_abi(ir: &str) -> String {
    adapt_with(ir, table())
}

fn adapt_with(ir: &str, table: &HashMap<&str, Sig>) -> String {
    let mut out = String::with_capacity(ir.len() + 4096);
    // (symbol, call ret, call arg types) -> adapter name
    let mut adapters: HashMap<(String, String, Vec<String>), String> = HashMap::new();
    let mut defs = String::new();
    for line in ir.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let nl = &line[body.len()..];
        if let Some((name, suffix)) = parse_declare(body) {
            if let Some(sig) = table.get(name) {
                let (ret, params) = sig.ir_shape();
                let _ = write!(
                    out,
                    "declare {ret} @{name}({}){suffix}{nl}",
                    params.join(", ")
                );
                continue;
            }
        }
        if let Some(site) = parse_call(body) {
            if let Some(sig) = table.get(site.name) {
                let (real_ret, real_params) = sig.ir_shape();
                let call_args: Vec<&str> = site.args.iter().map(|(t, _)| *t).collect();
                if site.ret != real_ret || call_args != real_params {
                    let key = (
                        site.name.to_string(),
                        site.ret.to_string(),
                        call_args.iter().map(|s| s.to_string()).collect(),
                    );
                    let next = adapters.len();
                    let adapter_name = adapters
                        .entry(key)
                        .or_insert_with(|| {
                            let an = format!("__perry_wabi.{}.{next}", site.name);
                            defs.push_str(&adapter(site.name, sig, site.ret, &call_args, &an));
                            an
                        })
                        .clone();
                    let _ = write!(out, "{}{adapter_name}{}{nl}", site.head, site.tail);
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    if !defs.is_empty() {
        out.push_str("\n; wasm32 runtime ABI adapters (#11378)\n");
        out.push_str(&defs);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl() -> HashMap<&'static str, Sig> {
        parse_table(
            "# header\n\
             js_ptr_ret\tptr\tptr,i32u\n\
             js_usize_ret\tusize\t\n\
             js_signed\ti32s\tf64\n\
             js_takes_ptr\tvoid\tptr\n\
             js_two\tf64\tf64,f64\n",
        )
    }

    #[test]
    fn matching_calls_and_foreign_symbols_are_untouched() {
        let ir = "declare double @js_two(double, double)\n\
                  declare i64 @user_fn(i64)\n\
                  define void @f() {\n\
                  entry:\n  %1 = call double @js_two(double 1.0, double 2.0)\n  %2 = call i64 @user_fn(i64 3)\n  ret void\n}\n";
        assert_eq!(adapt_with(ir, &tbl()), ir);
    }

    #[test]
    fn i64_pointer_calls_get_one_adapter_per_shape() {
        let ir = "declare i64 @js_ptr_ret(i64, i32)\n\
                  declare void @js_takes_ptr(i64)\n\
                  define void @f(i64 %h) {\n\
                  entry:\n  %1 = call i64 @js_ptr_ret(i64 %h, i32 4)\n  %2 = call i64 @js_ptr_ret(i64 %1, i32 5)\n  call void @js_takes_ptr(i64 %2)\n  ret void\n}\n";
        let out = adapt_with(ir, &tbl());
        // Declarations become the runtime's real signatures.
        assert!(out.contains("declare ptr @js_ptr_ret(ptr, i32)\n"), "{out}");
        assert!(out.contains("declare void @js_takes_ptr(ptr)\n"), "{out}");
        // Both calls of the same shape share one adapter.
        assert_eq!(
            out.matches("@__perry_wabi.js_ptr_ret.0(").count(),
            3,
            "{out}"
        );
        assert!(out.contains("inttoptr i64 %a0 to ptr"), "{out}");
        assert!(out.contains("ptrtoint ptr %r to i64"), "{out}");
        assert!(
            out.contains("call void @__perry_wabi.js_takes_ptr.1(i64 %2)"),
            "{out}"
        );
    }

    #[test]
    fn narrow_returns_widen_by_the_runtime_types_signedness() {
        let ir = "  %1 = call i64 @js_usize_ret()\n  %2 = call i64 @js_signed(double %x)\n";
        let out = adapt_with(ir, &tbl());
        assert!(
            out.contains("zext i32 %r to i64"),
            "usize widens unsigned: {out}"
        );
        assert!(
            out.contains("sext i32 %r to i64"),
            "i32s widens signed: {out}"
        );
    }

    #[test]
    fn invoke_tail_and_short_calls_are_adapted() {
        let ir = "  %1 = invoke double @js_two(double %a) to label %ok unwind label %lp\n\
                  \x20 %2 = tail call double @js_two(i64 %b, double %c)\n\
                  \x20 call void @js_signed(double %d)\n";
        let out = adapt_with(ir, &tbl());
        // Missing argument: a defined zero, not garbage.
        assert!(
            out.contains("call double @js_two(double %a0, double 0.0)"),
            "{out}"
        );
        assert!(
            out.contains(
                "invoke double @__perry_wabi.js_two.0(double %a) to label %ok unwind label %lp"
            ),
            "{out}"
        );
        assert!(
            out.contains("tail call double @__perry_wabi.js_two.1(i64 %b, double %c)"),
            "{out}"
        );
        assert!(out.contains("bitcast i64 %a0 to double"), "{out}");
        // A value dropped by a void call site.
        assert!(
            out.contains("call void @__perry_wabi.js_signed.2(double %d)"),
            "{out}"
        );
    }

    #[test]
    fn the_checked_in_table_parses() {
        let t = table();
        assert!(
            t.len() > 1000,
            "runtime_abi.tsv looks truncated: {} symbols",
            t.len()
        );
        let s = &t["js_string_from_bytes"];
        assert_eq!(s.ir_shape(), ("ptr", vec!["ptr", "i32"]));
    }
}

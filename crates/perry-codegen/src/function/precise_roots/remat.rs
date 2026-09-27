//! Rematerialize global-backed roots instead of relocating them.
//!
//! # What this removes
//!
//! A root slot whose only heap value is a copy of a GC-root *global* does not
//! need to be a root. On the Claude Code bundle's largest function
//! (`__25747`, 24% of every RS4GC relocation in the bundle) 1,326 of its 1,721
//! root allocas were exactly that, and they produced 90% of its 3.3 M
//! relocations:
//!
//! - **1,171 named locals holding a string literal** — `var X = "…"` stores
//!   `load double, ptr @<mod>_.str.N.handle` into the local's slot, and the
//!   slot then lives (and is relocated at every statepoint) until the function
//!   returns;
//! - **155 entry-hoisted class-keys caches** — `entry_init_load_rooted_global`
//!   copies `@perry_class_keys_*` into a slot once per function (#7876).
//!
//! Both globals are registered roots the collector marks AND rewrites on
//! evacuation, and neither is ever reassigned after module init. So the value a
//! relocation would have produced is, at every point, simply the global's
//! current value — one load, no spill, no `gc.relocate`, no stack-map entry.
//!
//! # The transform (native RS4GC lowering only)
//!
//! For an eligible slot `S` backed by global `@G`:
//!
//! ```llvm
//! store double %h, ptr %S         ; %h = load double, ptr @G
//! ; becomes
//! store double 0x7FFC000000004D52, ptr %S   ; REMAT_MARK: "holds @G's value"
//!
//! %r = load double, ptr %S
//! ; becomes
//! %r.rmv = load double, ptr %S
//! %r.rmb = bitcast double %r.rmv to i64
//! %r.rmk = icmp eq i64 %r.rmb, <REMAT_MARK>
//! %r.rmg = load double, ptr @G                ; re-read AT THE USE
//! %r = select i1 %r.rmk, double %r.rmg, double %r.rmv
//! ```
//!
//! and the slot stays a plain alloca: no `ptr addrspace(1)` retype, so RS4GC
//! never sees it. After `mem2reg` the slot is a phi of constants (the marker,
//! `undefined`, …); where the `@G` store dominates the use the select folds and
//! the whole read is the single `load @G`. The marker lives in the reserved
//! `0x7FFC` special-constant band, so it is never a heap reference and never a
//! value user code can observe (every read of the slot is rewritten).
//!
//! # Correctness conditions (all checked here, on the emitted IR)
//!
//! A slot is rematerialized only if ALL of these hold; otherwise it stays an
//! ordinary relocated root, byte-for-byte the previous lowering:
//!
//! 1. **The global is a registered root the collector rewrites.** Only two
//!    families qualify, matched by name ([`is_rematerializable_global`]):
//!    string-literal handles (`__perry_init_strings_*` stores each one and
//!    immediately calls `js_gc_register_global_root` on it,
//!    `codegen/string_pool.rs`) and `@perry_class_keys_*` (registered the same
//!    way, #5042). Evacuation and old-page defrag rewrite the global in place,
//!    which is what makes a fresh load equal to the relocated copy.
//! 2. **The global is never reassigned after initialization.** Each is written
//!    exactly once, by its module's init. `@perry_global_*` (module `let`/`var`
//!    bindings) and `@perry_static_*` (static fields) are registered roots too,
//!    but the PROGRAM assigns them, so a re-read could observe a later
//!    assignment; they are deliberately not matched. As a belt-and-braces check
//!    a function that itself stores to `@G` disqualifies `@G`.
//! 3. **The global is initialized before the use.** The string pool and class
//!    keys are built at the top of the module's init (`__perry_init_strings_*`),
//!    before any user code of that module runs, and every stored value was
//!    itself a load of `@G`, so the store already observed the initialized
//!    global. (In an import cycle that runs a function before its module's init
//!    the stored value would be the uninitialized `0`; the re-read would then
//!    see the initialized global, which is the value the program was supposed to
//!    have. It never observes a *different* initialized object.)
//! 4. **Every store into the slot is either a load of that one `@G` (through
//!    bit-preserving `bitcast`s only) or a constant**, and every other mention
//!    of the slot is its `alloca`, its shadow bind, or a whole `load`. A constant
//!    is never a heap reference, so it needs no root either; it is kept as is.
//!    Any other shape — a call result, a phi, a second global, the slot's
//!    address escaping — leaves the slot a root.
//!
//! **Worker programs** (#10399): with a `new Worker` in the program the handle
//! and class-keys globals are `thread_local`, each thread runs its own module
//! init and registers its own copy with its own collector. A function never
//! migrates threads, so the re-read names the same thread's copy the stored
//! value came from; the condition set above holds per thread unchanged.
//!
//! **Shadow-stack lowering** (targets without native roots) is untouched: there
//! the collector rewrites the bound alloca in place and nothing is relocated.
//!
//! # Why reloading beats relocating
//!
//! A relocated root costs, at EVERY statepoint it is live across, one stack-map
//! entry, one `gc.relocate`, and in practice a spill and a reload; for a
//! function-lifetime local that is every call in the function. A rematerialized
//! value costs one load of `@G` per *use*. The statepoint side is also where
//! RS4GC's compile time goes (`__25747`: 142k → 3.66M instructions, 125 s).

use std::collections::{HashMap, HashSet};

/// The "slot currently holds `@G`'s value" marker. In the reserved `0x7FFC`
/// special-constant band beside `undefined`/`null`/`true`/`false`/hole/TDZ
/// (`nanbox.rs`), and none of those. Never a heap reference, never observable.
pub(crate) const REMAT_MARK: u64 = 0x7FFC_0000_0000_4D52;

/// One slot's rematerialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Plan {
    /// The backing global, sigil included (`@m_.str.3.handle`).
    pub global: String,
    /// The type the global is loaded as (`double` for a string handle, `i64`
    /// for a class-keys array pointer), exactly as the original load read it.
    pub global_ty: &'static str,
}

/// Is `name` (no `@`) a string-literal handle global — `<mod>_.str.<N>.handle`,
/// or `.str.<N>.handle` for a pool built without a module prefix?
///
/// Same family `root_reload.rs::is_string_handle_global` reloads (#7664); the
/// prefix-less form is accepted here too because it is the same pool with an
/// empty `module_prefix` (`strings.rs`).
fn is_string_handle_global(name: &str) -> bool {
    let Some(rest) = name.strip_suffix(".handle") else {
        return false;
    };
    let digits = if let Some(d) = rest.strip_prefix(".str.") {
        d
    } else {
        match rest.rsplit_once("_.str.") {
            Some((head, d)) if !head.is_empty() => d,
            _ => return false,
        }
    };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Is `name` (no `@`) a per-class canonical keys-array global? Excludes the
/// packed rodata the array is built from (`perry_class_keys_packed_*`), which
/// is a constant byte string and never a heap reference.
fn is_class_keys_global(name: &str) -> bool {
    name.starts_with("perry_class_keys_") && !name.starts_with("perry_class_keys_packed_")
}

/// Conditions 1–2's name half: registered, rewritten on evacuation, written
/// only by module init. See the module docs for why nothing else qualifies.
pub(crate) fn is_rematerializable_global(name: &str) -> bool {
    is_string_handle_global(name) || is_class_keys_global(name)
}

fn is_local_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '%' || c == '_' || c == '.'
}

fn value_ty(ty: &str) -> Option<&'static str> {
    match ty {
        "i64" => Some("i64"),
        "double" => Some("double"),
        _ => None,
    }
}

/// The bit pattern of a constant store operand, or `None` if `v` is not a
/// constant this pass understands. Only used to prove the constant is not the
/// marker; every constant is non-heap by construction (addresses are runtime).
fn constant_bits(ty: &str, v: &str) -> Option<u64> {
    match ty {
        "i64" => v
            .parse::<i64>()
            .map(|n| n as u64)
            .ok()
            .or_else(|| v.parse::<u64>().ok()),
        "double" => {
            if let Some(hex) = v.strip_prefix("0x") {
                if hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return u64::from_str_radix(hex, 16).ok();
                }
                return None;
            }
            v.parse::<f64>().ok().map(f64::to_bits)
        }
        _ => None,
    }
}

/// `R = load T, ptr P` → `(R, T, P)`.
fn parse_load(trimmed: &str) -> Option<(&str, &str, &str)> {
    let (result, rest) = trimmed.split_once(" = load ")?;
    if result.contains(' ') || !result.starts_with('%') {
        return None;
    }
    let (ty, ptr) = rest.split_once(", ptr ")?;
    let ptr = ptr.trim();
    if ptr.contains(|c: char| c == ',' || c == ' ') {
        return None;
    }
    Some((result, ty.trim(), ptr))
}

/// `store T V, ptr P` → `(T, V, P)`.
fn parse_store(trimmed: &str) -> Option<(&str, &str, &str)> {
    let rest = trimmed.strip_prefix("store ")?;
    let (ty, rest) = rest.split_once(' ')?;
    let (value, ptr) = rest.rsplit_once(", ptr ")?;
    let ptr = ptr.trim();
    if ptr.contains(|c: char| c == ',' || c == ' ') {
        return None;
    }
    Some((ty, value.trim(), ptr))
}

fn parse_bind_ptr(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("call void @js_shadow_slot_bind(i32 ")?;
    let (_, ptr) = rest.split_once(", ptr ")?;
    Some(ptr.strip_suffix(')')?.trim())
}

/// How a register was defined, in the two shapes a global-backed value can
/// take between its load and its store.
enum Def<'a> {
    /// `load T, ptr @G` — `(T, "@G")`.
    GlobalLoad(&'a str, &'a str),
    /// `bitcast A %x to B` between `i64` and `double`.
    Bitcast(&'a str),
}

fn parse_def(rhs: &str) -> Option<Def<'_>> {
    if let Some(rest) = rhs.strip_prefix("load ") {
        let (ty, ptr) = rest.split_once(", ptr ")?;
        let ptr = ptr.trim();
        let ty = value_ty(ty.trim())?;
        if ptr.starts_with('@') && !ptr.contains(|c: char| c == ',' || c == ' ') {
            return Some(Def::GlobalLoad(ty, ptr));
        }
        return None;
    }
    let rest = rhs.strip_prefix("bitcast ")?;
    let (src_ty, rest) = rest.split_once(' ')?;
    let (src, dst_ty) = rest.split_once(" to ")?;
    if value_ty(src_ty).is_some() && value_ty(dst_ty.trim()).is_some() && src.starts_with('%') {
        return Some(Def::Bitcast(src.trim()));
    }
    None
}

/// Follow bit-preserving casts back to a global load: `Some((T, "@G"))`.
fn resolve_global<'a>(defs: &HashMap<&'a str, Def<'a>>, mut reg: &'a str) -> Option<(&'a str, &'a str)> {
    for _ in 0..8 {
        match defs.get(reg)? {
            Def::GlobalLoad(ty, g) => return Some((ty, g)),
            Def::Bitcast(src) => reg = src,
        }
    }
    None
}

/// Decide which of `root_ptrs` are backed by one immutable rooted global.
/// Pure analysis; [`apply`] does the rewrite.
pub(super) fn plan(lines: &[&str], root_ptrs: &[String]) -> HashMap<String, Plan> {
    let roots: HashSet<&str> = root_ptrs.iter().map(String::as_str).collect();
    if roots.is_empty() {
        return HashMap::new();
    }
    let mut defs: HashMap<&str, Def<'_>> = HashMap::new();
    let mut stored_globals: HashSet<&str> = HashSet::new();
    let mut disqualified: HashSet<&str> = HashSet::new();
    // Per root: the store operands `(ty, value)`.
    let mut stores: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();

    for line in lines {
        let trimmed = line.trim();
        if let Some((result, rhs)) = trimmed.split_once(" = ") {
            if result.starts_with('%') && !result.contains(' ') {
                if let Some(def) = parse_def(rhs) {
                    defs.insert(result, def);
                }
            }
        }
        // Classify the one root access this line may be. Each recognized shape
        // names its root exactly once, as the pointer operand.
        let mut accessed: Option<&str> = None;
        if let Some(ptr) = parse_bind_ptr(trimmed) {
            accessed = roots.contains(ptr).then_some(ptr);
        } else if let Some((ty, value, ptr)) = parse_store(trimmed) {
            if ptr.starts_with('@') {
                stored_globals.insert(ptr);
            }
            if roots.contains(ptr) {
                if value_ty(ty).is_some() && !roots.contains(value) {
                    stores.entry(ptr).or_default().push((ty, value));
                    accessed = Some(ptr);
                } else {
                    disqualified.insert(ptr);
                }
            }
        } else if let Some((_, ty, ptr)) = parse_load(trimmed) {
            if roots.contains(ptr) {
                if value_ty(ty).is_some() {
                    accessed = Some(ptr);
                } else {
                    disqualified.insert(ptr);
                }
            }
        } else if let Some((reg, rhs)) = trimmed.split_once(" = ") {
            if roots.contains(reg) && matches!(rhs.trim(), "alloca i64" | "alloca double") {
                accessed = Some(reg);
            }
        }
        // Fail closed: any mention of a root beyond the one recognized access
        // (its address passed to a call, a GEP, a second operand, …).
        let mut seen_access = false;
        for tok in line.split(|c: char| !is_local_name_char(c)) {
            if !roots.contains(tok) {
                continue;
            }
            if Some(tok) == accessed && !seen_access {
                seen_access = true;
                continue;
            }
            disqualified.insert(tok);
        }
    }

    let mut plans = HashMap::new();
    for root in root_ptrs {
        let root = root.as_str();
        if disqualified.contains(root) {
            continue;
        }
        let Some(root_stores) = stores.get(root) else {
            continue;
        };
        let mut backing: Option<(&str, &str)> = None;
        let mut ok = true;
        for &(ty, value) in root_stores {
            if value.starts_with('%') {
                match resolve_global(&defs, value) {
                    Some((gty, g))
                        if is_rematerializable_global(&g[1..])
                            && !stored_globals.contains(g)
                            && backing.is_none_or(|(_, b)| b == g) =>
                    {
                        backing = Some((gty, g));
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            } else if constant_bits(ty, value).is_none_or(|bits| bits == REMAT_MARK) {
                ok = false;
                break;
            }
        }
        if let (true, Some((gty, g))) = (ok, backing) {
            plans.insert(
                root.to_string(),
                Plan {
                    global: g.to_string(),
                    global_ty: value_ty(gty).expect("parse_def only admits i64/double"),
                },
            );
        }
    }
    plans
}

fn mark_literal(ty: &str) -> String {
    match ty {
        "double" => format!("0x{REMAT_MARK:016X}"),
        _ => (REMAT_MARK as i64).to_string(),
    }
}

/// Rewrite every access to a planned slot (see the module docs) and return
/// the new function text. Lines not touching a planned slot pass through.
pub(super) fn apply(lines: &[&str], plans: &HashMap<String, Plan>) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum::<usize>() + plans.len() * 256);
    let mark_i64 = (REMAT_MARK as i64).to_string();
    for line in lines {
        let trimmed = line.trim();
        if let Some((reg, rhs)) = trimmed.split_once(" = ") {
            if plans.contains_key(reg) {
                if let Some(ty) = rhs.trim().strip_prefix("alloca ").and_then(value_ty) {
                    // mem2reg needs a dominating definition on read-before-store
                    // paths, same reason the root form null-inits its slots.
                    let zero = if ty == "double" { "0.0" } else { "0" };
                    out.push_str(line);
                    out.push('\n');
                    out.push_str(&format!("  store {ty} {zero}, ptr {reg}\n"));
                    continue;
                }
            }
        }
        if let Some((ty, value, ptr)) = parse_store(trimmed) {
            if plans.contains_key(ptr) && value.starts_with('%') {
                // The value is `@G`'s; record THAT, not the soon-stale address.
                out.push_str(&format!("  store {ty} {}, ptr {ptr}\n", mark_literal(ty)));
                continue;
            }
        }
        if let Some((result, ty, ptr)) = parse_load(trimmed) {
            if let Some(plan) = plans.get(ptr) {
                let bits = if ty == "double" {
                    out.push_str(&format!("  {result}.rmv = load double, ptr {ptr}\n"));
                    out.push_str(&format!("  {result}.rmb = bitcast double {result}.rmv to i64\n"));
                    format!("{result}.rmb")
                } else {
                    out.push_str(&format!("  {result}.rmv = load i64, ptr {ptr}\n"));
                    format!("{result}.rmv")
                };
                out.push_str(&format!("  {result}.rmk = icmp eq i64 {bits}, {mark_i64}\n"));
                out.push_str(&format!(
                    "  {result}.rmg = load {}, ptr {}\n",
                    plan.global_ty, plan.global
                ));
                let current = if plan.global_ty == ty {
                    format!("{result}.rmg")
                } else {
                    out.push_str(&format!(
                        "  {result}.rmc = bitcast {} {result}.rmg to {ty}\n",
                        plan.global_ty
                    ));
                    format!("{result}.rmc")
                };
                out.push_str(&format!(
                    "  {result} = select i1 {result}.rmk, {ty} {current}, {ty} {result}.rmv\n"
                ));
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests;

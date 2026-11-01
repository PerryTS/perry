//! The string methods that read their operands by ECMA-262's algorithm, and
//! the RegExp methods they dispatch to. Each rule is the spec's: which
//! operands are coerced (ToPrimitive), which are called (`GetMethod` of a
//! well-known symbol, with what `this` and arguments), what the callback of
//! `replace` is given, and the result type. A method a program replaced
//! (`intrinsic_miss`) still runs what was stored, beside the rule.

use super::collections::cb_call;
use super::*;

const WK: &str = collections::WK_PREFIX;

/// Operand `v` is read by ToPrimitive (ToString / ToNumber / ToPropertyKey).
pub(crate) fn to_prim(m: &mut Model, v: NodeId, why: &str) {
    let r = m.reason(why);
    m.cons(
        v,
        Cons::Implicit {
            reason: r,
            proto: Protocol::ToPrimitive,
        },
    );
}

/// `Get(recv, key)`: a getter runs; the value is read as a primitive.
pub(crate) fn read_prim(m: &mut Model, recv: NodeId, key: &str, why: &str) {
    let a = m.atom(key);
    let dst = m.node_l("read");
    m.cons(
        recv,
        Cons::Load {
            key: Key::Named(a),
            dst,
        },
    );
    to_prim(m, dst, why);
}

/// `Get(recv, key)` whose value is only tested (IsRegExp reads `@@match`).
pub(crate) fn read_only(m: &mut Model, recv: NodeId, key: &str) {
    let a = m.atom(key);
    let dst = m.node_l("read");
    m.cons(
        recv,
        Cons::Load {
            key: Key::Named(a),
            dst,
        },
    );
}

/// A call the builtin makes while it runs: `recv[key](...args)`.
fn method_call(m: &mut Model, by: CallId, recv: NodeId, key: &str, args: Vec<NodeId>) -> NodeId {
    let a = m.atom(key);
    let ret = m.node_l("builtin-ret");
    m.nodes[ret as usize].frame = m.call_run(by);
    let call = m.calls.len() as CallId;
    m.solver_call(
        by,
        CallSite {
            this: ThisArg::Node(recv),
            args,
            spread: vec![],
            ret,
            site: call,
        },
    );
    m.cons(
        recv,
        Cons::MCall {
            key: Key::Named(a),
            call,
        },
    );
    ret
}

fn sym_call(m: &mut Model, by: CallId, recv: NodeId, sym: &str, args: Vec<NodeId>) -> NodeId {
    method_call(m, by, recv, &format!("{WK}{sym}"), args)
}

fn str_node(m: &mut Model) -> NodeId {
    let n = m.node_l("str");
    m.add_prim(n, prim::STR);
    n
}

/// What the callback of `replace` is given (22.2.6.11 step 14, 22.1.3.19.1):
/// the match, the captures (strings or undefined), the offset, the string,
/// and the named-groups object when the pattern has groups.
fn replacer_arg(m: &mut Model, by: CallId) -> NodeId {
    let n = m.node_l("replacer-arg");
    m.add_prim(n, prim::STR | prim::NUM | prim::UNDEF);
    let g = site_tmpl(m, by, "groups");
    let fa = m.field_any(g);
    m.add_prim(fa, prim::STR | prim::UNDEF);
    m.add_tok(n, g);
    n
}

/// `replaceValue`: called (undefined this) when callable, with its result
/// ToString'd; otherwise ToString'd itself.
fn replace_value(m: &mut Model, by: CallId, rv: NodeId) {
    to_prim(m, rv, "replaceValue ToString");
    let a = replacer_arg(m, by);
    let r = cb_call(m, by, rv, ThisArg::Undef, vec![a; 5]);
    to_prim(m, r, "replace callback result ToString");
}

/// What `RegExp.prototype[@@x]` does with the match record `exec` returned:
/// reads `index`, `groups`, `length` and the captures, and the properties
/// of the groups object, each ToString'd or ToNumber'd.
fn read_match(m: &mut Model, rec: NodeId) {
    let v = m.node_l("match-field");
    m.cons(
        rec,
        Cons::Load {
            key: Key::Any,
            dst: v,
        },
    );
    to_prim(m, v, "match record field");
    let g = m.node_l("match-group");
    m.cons(
        v,
        Cons::Load {
            key: Key::Any,
            dst: g,
        },
    );
    to_prim(m, g, "match record group");
}

/// A string method of a primitive receiver, by its algorithm. `false`: no
/// rule (the method keeps the generic summary).
pub(super) fn string_method(m: &mut Model, call: CallId, name: &str) -> bool {
    let c = m.calls[call as usize].clone();
    if !c.spread.is_empty() {
        return false;
    }
    let arg0 = c.args.first().copied();
    let s = str_node(m);
    match name {
        // 22.1.3.19 / .20: GetMethod(searchValue, @@replace) called with
        // (O, replaceValue); else the search string, ToString'd, and
        // replaceValue called with (match, position, string). replaceAll
        // first tests IsRegExp(searchValue) and reads its `flags`.
        "replace" | "replaceAll" => {
            let (Some(p), Some(rv)) = (arg0, c.args.get(1).copied()) else {
                for &a in &c.args {
                    to_prim(m, a, "String.prototype.replace operand");
                }
                ret_prim(m, call, prim::STR);
                return true;
            };
            to_prim(m, p, "replace searchValue ToString");
            if name == "replaceAll" {
                read_only(m, p, &format!("{WK}match"));
                read_prim(m, p, "flags", "replaceAll flags ToString");
            }
            replace_value(m, call, rv);
            let r = sym_call(m, call, p, "replace", vec![s, rv]);
            m.edge(r, c.ret);
            ret_prim(m, call, prim::STR);
        }
        // 22.1.3.23: GetMethod(separator, @@split) called with (O, limit);
        // else ToString(separator), ToUint32(limit).
        "split" => {
            if let Some(p) = arg0 {
                to_prim(m, p, "split separator ToString");
                let lim = c.args.get(1).copied().unwrap_or(s);
                to_prim(m, lim, "split limit ToUint32");
                let r = sym_call(m, call, p, "split", vec![s, lim]);
                m.edge(r, c.ret);
            }
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
        }
        // 22.1.3.13 / .14: match: GetMethod(regexp, @@match) called with
        // (O), else RegExpCreate(regexp): ToString(regexp). matchAll first
        // tests IsRegExp and reads `flags`, then calls @@matchAll.
        "match" | "matchAll" => {
            if let Some(p) = arg0 {
                to_prim(m, p, "RegExpCreate pattern ToString");
                if name == "matchAll" {
                    read_only(m, p, &format!("{WK}match"));
                    read_prim(m, p, "flags", "matchAll flags ToString");
                }
                let r = sym_call(m, call, p, name, vec![s]);
                m.edge(r, c.ret);
            }
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
            if name == "match" {
                ret_prim(m, call, prim::NULL);
            }
        }
        // 22.1.3.22: @@search called with (O), else RegExpCreate.
        "search" => {
            if let Some(p) = arg0 {
                to_prim(m, p, "RegExpCreate pattern ToString");
                let r = sym_call(m, call, p, "search", vec![s]);
                m.edge(r, c.ret);
            }
            ret_prim(m, call, prim::NUM);
        }
        // Every operand is ToString'd or ToIntegerOrInfinity'd.
        "charAt" | "charCodeAt" | "codePointAt" | "at" | "slice" | "substring" | "substr"
        | "padStart" | "padEnd" | "repeat" | "concat" | "normalize" | "trim" | "trimStart"
        | "trimEnd" | "indexOf" | "lastIndexOf" | "toFixed" | "toPrecision" | "toExponential" => {
            for &a in &c.args {
                to_prim(m, a, "string method operand");
            }
            ret_prim(
                m,
                call,
                match name {
                    "charCodeAt" | "indexOf" | "lastIndexOf" => prim::NUM,
                    "codePointAt" => prim::NUM | prim::UNDEF,
                    "at" => prim::STR | prim::UNDEF,
                    _ => prim::STR,
                },
            );
        }
        // 22.1.3.7 / .24 / .8 step 2: IsRegExp(searchString) reads @@match;
        // then ToString.
        "includes" | "startsWith" | "endsWith" => {
            if let Some(p) = arg0 {
                read_only(m, p, &format!("{WK}match"));
            }
            for &a in &c.args {
                to_prim(m, a, "string search operand");
            }
            ret_prim(m, call, prim::BOOL);
        }
        // 22.1.3.12: `that` is ToString'd; locales and options are read by
        // Intl (any key): they keep the generic protocol.
        "localeCompare" => {
            if let Some(t) = arg0 {
                to_prim(m, t, "localeCompare operand");
            }
            let r = m.reason("localeCompare locales/options");
            for &a in c.args.iter().skip(1) {
                m.cons(
                    a,
                    Cons::Implicit {
                        reason: r,
                        proto: Protocol::Any,
                    },
                );
            }
            ret_prim(m, call, prim::NUM);
        }
        _ => return false,
    }
    true
}

/// The methods of a RegExp object this module describes.
pub(super) fn regexp_described(name: &str) -> bool {
    matches!(
        name,
        "exec" | "test" | "@@__perry_wk_replace" | "@@__perry_wk_match" | "@@__perry_wk_search"
    )
}

/// `RegExp.prototype` methods (22.2.6). Each runs the object's `exec`
/// through a property lookup (RegExpExec), so a replaced or overridden
/// `exec` is called; the builtin `exec` reads `lastIndex` and ToString's
/// its argument. `@@split` and `@@matchAll` construct through the species
/// constructor and keep the generic summary.
pub(super) fn regexp_method(m: &mut Model, t: Tok, name: &str, call: CallId) {
    let c = m.calls[call as usize].clone();
    let recv = m.node_l("regexp");
    m.add_tok(recv, t);
    if let Some(s) = c.args.first().copied() {
        to_prim(m, s, "RegExp argument ToString");
    }
    match name {
        "exec" => {
            read_prim(m, recv, "lastIndex", "RegExp lastIndex ToLength");
            let pb = site_tmpl(m, call, "primbox");
            ret_tok(m, call, pb);
            ret_prim(m, call, prim::NULL);
        }
        _ => {
            if let Some(s) = c.args.first().copied() {
                read_prim(m, recv, "flags", "RegExp flags ToString");
                let r = method_call(m, call, recv, "exec", vec![s]);
                read_match(m, r);
            }
            match name {
                "test" => ret_prim(m, call, prim::BOOL),
                "@@__perry_wk_replace" => {
                    if let Some(&rv) = c.args.get(1) {
                        replace_value(m, call, rv);
                    }
                    ret_prim(m, call, prim::STR);
                }
                "@@__perry_wk_match" => {
                    let pb = site_tmpl(m, call, "primbox");
                    ret_tok(m, call, pb);
                    ret_prim(m, call, prim::NULL);
                }
                _ => ret_prim(m, call, prim::NUM),
            }
        }
    }
}

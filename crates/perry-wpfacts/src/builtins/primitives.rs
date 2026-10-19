//! Primitive method argument roles and conversion protocols.
use super::*;

/// Conversion invokes the object's conversion hooks, with primitive hints.
/// In particular, converting a function does not invoke that function.
pub fn coerce(m: &mut Model, t: Tok, _reason: u32) {
    // Escape already connects every reachable method with this=t and
    // arbitrary primitive arguments, and touches all results. It strictly
    // contains the effects of these narrower conversion calls.
    if t == TOP || m.is_escaped(t) {
        return;
    }
    let hint = m.node_l("coerce-hint");
    m.add_prim(hint, prim::STR);
    for name in ["toString", "valueOf", "@@__perry_wk_toPrimitive"] {
        let ret = m.coercion_results;
        let call = m.calls.len() as CallId;
        m.calls.push(CallSite {
            this: ThisArg::Tok(t),
            args: if name.ends_with("toPrimitive") {
                vec![hint]
            } else {
                vec![]
            },
            spread: vec![],
            ret,
            // A single synthetic protocol origin, distinct from source
            // calls, keeps derived builtin allocations finite on cycles.
            site: u32::MAX,
        });
        let key = Key::Named(m.atom(name));
        let n = m.node_l("coerce-object");
        m.add_tok(n, t);
        m.cons(n, Cons::MCall { key, call });
    }
}

/// Only the argument positions actually converted by the builtin. Extra
/// arguments are ignored, including functions passed in those positions.
pub(super) fn coerce_args(m: &mut Model, call: CallId, positions: &[usize]) {
    let c = m.calls[call as usize].clone();
    let reason = m.reason("builtin argument conversion");
    for &i in positions {
        if let Some(&a) = c.args.get(i) {
            m.cons(a, Cons::Coerce { reason });
        }
    }
    // A spread may supply any converted position.
    if !positions.is_empty() {
        let ek = m.elem_key();
        for &s in &c.spread {
            let a = m.node_l("coerce-spread");
            m.cons(s, Cons::Load { key: ek, dst: a });
            m.cons(a, Cons::Coerce { reason });
        }
    }
}

pub fn prim_method(m: &mut Model, prim: u16, key: Key, call: CallId) {
    if prim & (prim::NULL | prim::UNDEF) == prim {
        return;
    }
    if !m.calls[call as usize].spread.is_empty() {
        return unknown_method(m, call, "spread arguments of primitive method");
    }
    // All builtin prototypes share this token. Written members already
    // escape on store; follow their values as well as the intrinsic summary.
    let proto = m.tok(TokKind::ObjectProto);
    let f = match key {
        Key::Named(a) => m.field(proto, a),
        Key::Any => m.all_fields(proto),
    };
    m.cons(f, Cons::Call { call });
    watch_override(m, proto, key, call);
    m.accessor_hit(proto, proto, key, ChainAct::Call(call));
    let Key::Named(a) = key else {
        return unknown_method(m, call, "computed primitive method");
    };
    let name = m.atoms[a as usize].clone();
    match name.as_str() {
        "@@__perry_wk_toPrimitive" if prim & prim::SYMBOL != 0 => ret_prim(m, call, prim::SYMBOL),
        "valueOf" => ret_prim(m, call, prim & !(prim::NULL | prim::UNDEF)),
        "hasOwnProperty" | "propertyIsEnumerable" => {
            coerce_args(m, call, &[0]);
            ret_prim(m, call, prim::BOOL);
        }
        "isPrototypeOf" => ret_prim(m, call, prim::BOOL),
        "toString" | "toLocaleString" => {
            coerce_args(m, call, &[0, 1]);
            if name == "toLocaleString" {
                read_options(m, call, 1);
            }
            ret_prim(m, call, prim::STR);
        }
        "toFixed" | "toExponential" | "toPrecision" if prim & prim::NUM != 0 => {
            coerce_args(m, call, &[0]);
            ret_prim(m, call, prim::STR);
        }
        // Absent intrinsic methods throw. Prototype replacements above
        // still run (and retain their escaped-function TOP facts).
        _ => {}
    }
    if prim & prim::STR != 0 {
        string_method(m, &name, call);
    }
}

pub(super) fn string_method(m: &mut Model, name: &str, call: CallId) {
    match name {
        "replace" | "replaceAll" => {
            if let Some(search) = arg(m, call, 0) {
                // A RegExp's exec and protocol accessors may be user code.
                let reason = m.reason("string search object protocols");
                m.cons(search, Cons::Implicit { reason });
            }
            string_protocol(m, call, "replace", &[1]);
            coerce_args(m, call, &[0]);
            if let Some(cb) = arg(m, call, 1) {
                let text = m.node_l("replace-text");
                m.add_prim(text, prim::STR | prim::UNDEF);
                let num = m.node_l("replace-offset");
                m.add_prim(num, prim::NUM);
                // RegExp captures make the offset/input positions variable.
                // Spread contributes every possible capture and groups value
                // to every parameter, including a rest parameter.
                let groups = site_tmpl(m, call, "groups");
                let gf = m.field_any(groups);
                m.add_prim(gf, prim::STR | prim::UNDEF);
                let vals = site_tmpl(m, call, "Array");
                let vf = m.field_any(vals);
                m.edge(text, vf);
                m.edge(num, vf);
                m.add_tok(vf, groups);
                let spread = m.node_l("replace-args");
                m.add_tok(spread, vals);
                let ret = m.node_l("replace-result");
                let call2 = m.calls.len() as CallId;
                m.calls.push(CallSite {
                    this: ThisArg::Undef,
                    args: vec![text],
                    spread: vec![spread],
                    ret,
                    site: m.calls[call as usize].site,
                });
                m.cons(cb, Cons::Call { call: call2 });
                let reason = m.reason("replacement result conversion");
                m.cons(ret, Cons::Coerce { reason });
                coerce_args(m, call, &[1]);
            }
            ret_prim(m, call, prim::STR);
        }
        "split" | "match" | "matchAll" => {
            if let Some(search) = arg(m, call, 0) {
                let reason = m.reason("string search object protocols");
                m.cons(search, Cons::Implicit { reason });
            }
            string_protocol(m, call, name, if name == "split" { &[1] } else { &[] });
            coerce_args(m, call, &[0, 1]);
            let arr = site_tmpl(m, call, "Array");
            let elems = m.field_any(arr);
            m.add_prim(elems, prim::STR | prim::UNDEF);
            ret_tok(m, call, arr);
            if name != "split" {
                ret_prim(m, call, prim::NULL);
            }
        }
        "search" => {
            string_protocol(m, call, "search", &[]);
            coerce_args(m, call, &[0]);
            ret_prim(m, call, prim::NUM);
        }
        "charAt" | "at" | "charCodeAt" | "codePointAt" | "repeat" => {
            coerce_args(m, call, &[0]);
            ret_prim(
                m,
                call,
                if matches!(name, "charCodeAt" | "codePointAt") {
                    prim::NUM | prim::UNDEF
                } else {
                    prim::STR | prim::UNDEF
                },
            );
        }
        "slice" | "substring" | "substr" | "padStart" | "padEnd" => {
            coerce_args(m, call, &[0, 1]);
            ret_prim(m, call, prim::STR);
        }
        "includes" | "startsWith" | "endsWith" | "indexOf" | "lastIndexOf" | "localeCompare" => {
            coerce_args(m, call, &[0, 1, 2]);
            if name == "localeCompare" {
                read_options(m, call, 2);
            }
            ret_prim(
                m,
                call,
                if matches!(name, "includes" | "startsWith" | "endsWith") {
                    prim::BOOL
                } else {
                    prim::NUM
                },
            );
        }
        "concat" => {
            let n = m.calls[call as usize].args.len();
            coerce_args(m, call, &(0..n).collect::<Vec<_>>());
            ret_prim(m, call, prim::STR);
        }
        "toLowerCase" | "toUpperCase" | "toLocaleLowerCase" | "toLocaleUpperCase" | "trim"
        | "trimStart" | "trimEnd" | "trimLeft" | "trimRight" | "toWellFormed" => {
            ret_prim(m, call, prim::STR);
        }
        "anchor" | "fontcolor" | "fontsize" | "link" => {
            coerce_args(m, call, &[0]);
            ret_prim(m, call, prim::STR);
        }
        "big" | "blink" | "bold" | "fixed" | "italics" | "small" | "strike" | "sub" | "sup" => {
            ret_prim(m, call, prim::STR)
        }
        "normalize" => {
            coerce_args(m, call, &[0]);
            ret_prim(m, call, prim::STR);
        }
        "isWellFormed" => ret_prim(m, call, prim::BOOL),
        WK_ITERATOR => {
            let vals = m.node_l("string-iterated");
            m.add_prim(vals, prim::STR);
            let it = iter_over(m, call, vals);
            ret_tok(m, call, it);
        }
        _ => {}
    }
}

/// RegExp and user search objects receive the replacement/limit too.
fn string_protocol(m: &mut Model, call: CallId, protocol: &str, extra: &[usize]) {
    let Some(search) = arg(m, call, 0) else {
        return;
    };
    let text = m.node_l("string-input");
    m.add_prim(text, prim::STR);
    let mut args = vec![text];
    for &i in extra {
        if let Some(a) = arg(m, call, i) {
            args.push(a);
        } else {
            let u = m.node_l("undef");
            m.add_prim(u, prim::UNDEF);
            args.push(u);
        }
    }
    let ret = m.node_l("string-protocol-ret");
    let call2 = m.calls.len() as CallId;
    m.calls.push(CallSite {
        this: ThisArg::Node(search),
        args,
        spread: vec![],
        ret,
        site: m.calls[call as usize].site,
    });
    let key = Key::Named(m.atom(&format!("@@__perry_wk_{protocol}")));
    m.cons(search, Cons::MCall { key, call: call2 });
    // @@split / @@match / @@replace may return any value.
    let dst = m.calls[call as usize].ret;
    m.edge(ret, dst);
}

/// Static argument roles: arity `usize::MAX` means all arguments. This is
/// compiler data only; no runtime dispatch or summary table is emitted.
pub(super) fn static_method(m: &mut Model, ns: BuiltinNs, name: &str, call: CallId) -> bool {
    let (arity, result) = match (ns, name) {
        (BuiltinNs::Math, "random") => (0, prim::NUM),
        (BuiltinNs::Math, "max" | "min" | "hypot") => (usize::MAX, prim::NUM),
        (BuiltinNs::Math, "atan2" | "pow" | "imul") => (2, prim::NUM),
        (
            BuiltinNs::Math,
            "abs" | "acos" | "acosh" | "asin" | "asinh" | "atan" | "atanh" | "cbrt" | "ceil"
            | "clz32" | "cos" | "cosh" | "exp" | "expm1" | "floor" | "fround" | "f16round" | "log"
            | "log10" | "log1p" | "log2" | "round" | "sign" | "sin" | "sinh" | "sqrt" | "tan"
            | "tanh" | "trunc",
        ) => (1, prim::NUM),
        (BuiltinNs::Number, "parseInt") => (2, prim::NUM),
        (BuiltinNs::Number, "parseFloat") => (1, prim::NUM),
        (BuiltinNs::String, "fromCharCode" | "fromCodePoint") => (usize::MAX, prim::STR),
        (BuiltinNs::String, "raw") => {
            if let Some(template) = arg(m, call, 0) {
                let raw = m.node_l("raw-template");
                let key = Key::Named(m.atom("raw"));
                m.cons(template, Cons::Load { key, dst: raw });
                let values = m.node_l("raw-values");
                let key = m.elem_key();
                m.cons(raw, Cons::Load { key, dst: values });
                let reason = m.reason("String.raw template conversion");
                m.cons(values, Cons::Coerce { reason });
            }
            let n = m.calls[call as usize].args.len();
            coerce_args(m, call, &(1..n).collect::<Vec<_>>());
            ret_prim(m, call, prim::STR);
            return true;
        }
        _ => return false,
    };
    let n = if arity == usize::MAX {
        m.calls[call as usize].args.len()
    } else {
        arity
    };
    coerce_args(m, call, &(0..n).collect::<Vec<_>>());
    ret_prim(m, call, result);
    true
}

/// Locale/options processing reads properties and converts their values.
fn read_options(m: &mut Model, call: CallId, index: usize) {
    if let Some(options) = arg(m, call, index) {
        let values = m.node_l("builtin-options");
        m.cons(
            options,
            Cons::Load {
                key: Key::Any,
                dst: values,
            },
        );
        let reason = m.reason("builtin option conversion");
        m.cons(values, Cons::Coerce { reason });
    }
}

//! Constructor helper protocols, selected after normal callee resolution.
use super::*;

pub fn new_builtin_ns(m: &mut Model, ns: BuiltinNs, call: CallId) {
    m.diagnostic_builtin(call, |_| format!("new {ns:?}"));
    // A spread shifts argument positions, so a summary that reads fixed
    // positions cannot be trusted. Every fixed and spread value may land in
    // each position such a summary consumes (at most three), so rematch the
    // same site with that widened argument list instead of guessing.
    let c = m.calls[call as usize].clone();
    if !c.spread.is_empty() && ns != BuiltinNs::Array {
        let vals = all_arg_values(m, call);
        let c2 = m.calls.len() as CallId;
        m.calls.push(CallSite {
            this: ThisArg::Undef,
            args: vec![vals; c.args.len().max(3)],
            spread: vec![],
            ret: c.ret,
            site: c.site,
            region: m.thrown,
        });
        return new_builtin_ns(m, ns, c2);
    }
    match ns {
        BuiltinNs::Map | BuiltinNs::Set | BuiltinNs::WeakMap | BuiltinNs::WeakSet => {
            // The template names its collection kind (`members::collection_member`).
            let what = match ns {
                BuiltinNs::Map => "Map",
                BuiltinNs::Set => "Set",
                BuiltinNs::WeakMap => "WeakMap",
                _ => "WeakSet",
            };
            let c = site_tmpl(m, call, what);
            let fa = m.field_any(c);
            if ns == BuiltinNs::Map {
                // Born without an iterable, its contents are what `set`
                // stores (`solve/map_vals.rs`).
                m.map_born(c, arg(m, call, 0).is_none());
            }
            if let Some(src) = arg(m, call, 0) {
                let vals = iterate(m, call, src);
                if matches!(ns, BuiltinNs::Map | BuiltinNs::WeakMap) {
                    // Entries `[key, value]`: the collection holds both.
                    let ek = m.elem_key();
                    m.cons(vals, Cons::Load { key: ek, dst: fa });
                } else {
                    m.edge(vals, fa);
                }
            }
            ret_tok(m, call, c);
        }
        BuiltinNs::Promise => {
            let p = site_tmpl(m, call, "Promise");
            let v = promise_value(m, p);
            if let Some(ex) = arg(m, call, 0) {
                let res = resolver(m, v);
                let thrown = m.thrown;
                let rej = resolver(m, thrown);
                let rn = m.node_l("resolver");
                m.add_tok(rn, res);
                let jn = m.node_l("rejecter");
                m.add_tok(jn, rej);
                let ret = m.node_l("executor-ret");
                let call2 = m.calls.len() as CallId;
                m.solver_call(
                    call,
                    CallSite {
                        this: ThisArg::Undef,
                        args: vec![rn, jn],
                        spread: vec![],
                        ret,
                        site: call2,
                        region: m.thrown,
                    },
                );
                m.cons(ex, Cons::Call { call: call2 });
            }
            ret_tok(m, call, p);
        }
        BuiltinNs::Error | BuiltinNs::NativeError(_) | BuiltinNs::AggregateError => {
            implicit_args(m, call, "Error message", Protocol::Any);
            let e = error_tok(m, call);
            let options = if ns == BuiltinNs::AggregateError {
                if let Some(src) = arg(m, call, 0) {
                    let vals = iterate(m, call, src);
                    let arr = site_tmpl(m, call, "Array");
                    let elems = m.field_any(arr);
                    m.edge(vals, elems);
                    let errors = m.atom("errors");
                    let f = m.field(e, errors);
                    m.add_tok(f, arr);
                }
                2
            } else {
                1
            };
            if let Some(o) = arg(m, call, options) {
                let ca = m.atom("cause");
                let f = m.field(e, ca);
                m.cons(
                    o,
                    Cons::Load {
                        key: Key::Named(ca),
                        dst: f,
                    },
                );
            }
            ret_tok(m, call, e);
        }
        BuiltinNs::Date => {
            // ECMA-262 21.4.2.1: one operand ToPrimitive (unless a Date
            // internal-slot copy); 2..7 operands ToNumber. No operand is
            // retained or called otherwise; surplus operands are ignored.
            let c = m.calls[call as usize].clone();
            for &a in c.args.iter().take(if c.args.len() == 1 { 1 } else { 7 }) {
                string_methods::to_prim(m, a, "Date constructor ToNumber/ToString");
            }
            let o = site_tmpl(m, call, "Date");
            ret_tok(m, call, o);
        }
        BuiltinNs::RegExp => {
            // ECMA-262 22.2.4.1 RegExp(pattern, flags), 22.2.3.2
            // RegExpInitialize: IsRegExp reads `pattern[@@match]`; a pattern
            // without [[RegExpMatcher]] that IsRegExp accepts gives `source`
            // (and `flags` while `flags` is undefined); P and F go through
            // ToString. Nothing else of an operand is read or called (the
            // call arm adds the `constructor` read of `RegExp(re)`); surplus
            // operands are ignored.
            let c = m.calls[call as usize].clone();
            if let Some(&p) = c.args.first() {
                let wk = collections::WK_PREFIX;
                string_methods::read_only(m, p, &format!("{wk}match"));
                string_methods::read_prim(m, p, "source", "RegExp pattern source ToString");
                string_methods::read_prim(m, p, "flags", "RegExp pattern flags ToString");
                string_methods::to_prim(m, p, "RegExp pattern ToString");
            }
            if let Some(&f) = c.args.get(1) {
                string_methods::to_prim(m, f, "RegExp flags ToString");
            }
            let o = site_tmpl(m, call, "RegExp");
            ret_tok(m, call, o);
        }
        BuiltinNs::TypedArray
        | BuiltinNs::TypedArrayKind(_)
        | BuiltinNs::DataView
        | BuiltinNs::ArrayBuffer
        | BuiltinNs::SharedArrayBuffer => {
            // 23.2.5: object sources are iterated or read as array-like;
            // elements ToNumber/ToBigInt. ArrayBuffer length is ToIndex,
            // with only options.maxByteLength read. A DataView's buffer is
            // read through its internal slots; offset and length ToIndex.
            // No identity is kept.
            let buffer = matches!(ns, BuiltinNs::ArrayBuffer | BuiltinNs::SharedArrayBuffer);
            let what = match ns {
                BuiltinNs::DataView => "DataView",
                BuiltinNs::ArrayBuffer => "ArrayBuffer",
                BuiltinNs::SharedArrayBuffer => "SharedArrayBuffer",
                _ => "TypedArray",
            };
            let c = m.calls[call as usize].clone();
            if let Some(&src) = c.args.first() {
                if buffer {
                    string_methods::to_prim(m, src, "ArrayBuffer byteLength ToIndex");
                    if let Some(&opts) = c.args.get(1) {
                        string_methods::read_prim(
                            m,
                            opts,
                            "maxByteLength",
                            "ArrayBuffer maxByteLength ToIndex",
                        );
                    }
                } else if ns == BuiltinNs::DataView {
                    for &a in c.args.iter().skip(1).take(2) {
                        string_methods::to_prim(m, a, "DataView byteOffset/byteLength ToIndex");
                    }
                } else {
                    let vals = typed_array_methods::source(m, call, src);
                    string_methods::to_prim(
                        m,
                        vals,
                        "typed-array constructor element ToNumber/ToBigInt",
                    );
                    for &a in c.args.iter().skip(1).take(2) {
                        string_methods::to_prim(m, a, "typed-array byteOffset/length ToIndex");
                    }
                }
            }
            let o = site_tmpl(m, call, what);
            // A typed array's elements; a buffer or a DataView has none.
            if what == "TypedArray" {
                let fa = m.field_any(o);
                m.add_prim(fa, prim::NUM | prim::BIGINT);
            }
            let c = m.calls[call as usize].clone();
            if let Some(&src) = c.args.first() {
                m.cons(src, Cons::ShareBuffer { ta: o });
            }
            if buffer && c.args.len() > 1 {
                // B4: `new ArrayBuffer(n, { maxByteLength })` is resizable.
                let r = m.reason("B4 resizable ArrayBuffer");
                m.mark_hard(o, Hard::B4Resizable, r);
            }
            ret_tok(m, call, o);
        }
        BuiltinNs::Function => {
            if !function_ctor_runs_code(m, call, 0, true) {
                return;
            }
            set_dyn_eval(m, "E14 new Function(...)");
            unknown_method(m, call, "E14 dynamic code");
        }
        // Not a constructor (TypeError).
        BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_) => {}
        BuiltinNs::Array => call_builtin_ns(m, ns, call),
        BuiltinNs::Object => {
            let o = site_tmpl(m, call, "Object()");
            ret_tok(m, call, o);
        }
        _ => {
            implicit_args(m, call, "builtin constructor argument", Protocol::Any);
            let o = site_tmpl(m, call, "builtin");
            ret_tok(m, call, o);
        }
    }
}

/// `new Name(...)` where `Name` is not a program class: a builtin by name.
pub fn construct_named(m: &mut Model, name: &str, call: CallId) {
    m.diagnostic_builtin(call, |_| format!("new {name}"));
    if let Some(ns) = ns_by_name(name) {
        return new_builtin_ns(m, ns, call);
    }
    match name {
        "URL" => {
            let c = m.calls[call as usize].clone();
            for &a in c.args.iter().take(2) {
                string_methods::to_prim(m, a, "URL constructor USVString");
            }
            let o = site_tmpl(m, call, "URL");
            ret_tok(m, call, o);
        }
        "URLSearchParams" => {
            if let Some(src) = arg(m, call, 0) {
                // WebIDL union: primitive -> USVString (no hooks), object
                // -> iterable pairs or record. Coerce only entry values.
                let vals = iterate(m, call, src);
                let pair = iterate(m, call, vals);
                string_methods::to_prim(m, pair, "URLSearchParams pair USVString");
                let fields = m.node_l("params-record-value");
                m.cons(
                    src,
                    Cons::Load {
                        key: Key::any(),
                        dst: fields,
                    },
                );
                string_methods::to_prim(m, fields, "URLSearchParams record USVString");
            }
            let o = site_tmpl(m, call, "URLSearchParams");
            ret_tok(m, call, o);
        }
        "TextEncoder" | "TextDecoder" => {
            if name == "TextDecoder" {
                if let Some(label) = arg(m, call, 0) {
                    string_methods::to_prim(m, label, "TextDecoder label DOMString");
                }
                if let Some(opts) = arg(m, call, 1) {
                    for key in ["fatal", "ignoreBOM"] {
                        string_methods::read_only(m, opts, key);
                    }
                }
            }
            let o = site_tmpl(m, call, "TextCodec");
            ret_tok(m, call, o);
        }
        "Headers" => {
            implicit_args(m, call, "builtin constructor argument", Protocol::Any);
            let o = site_tmpl(m, call, "builtin");
            ret_tok(m, call, o);
        }
        "Proxy" => {
            let c = m.calls[call as usize].clone();
            let r = m.reason("E9 new Proxy (by name)");
            if let Some(&t) = c.args.first() {
                m.cons(
                    t,
                    Cons::Hard {
                        kind: Hard::E9Proxy,
                        reason: r,
                    },
                );
            }
            unknown_method(m, call, "E9 Proxy");
        }
        _ => match super::effects::declared_ctor(name) {
            // The realm's constructor, while the global holds it: what its
            // declaration says. A write under the name (now, or later in the
            // solve: the replay) is the unknown constructor again.
            Some(e) if !m.unit_mode => {
                let g = m.tok(TokKind::Builtin(BuiltinNs::GlobalThis));
                let a = m.atom(name);
                if native_written(m, g, a) {
                    unknown_method(m, call, &format!("argument of written constructor {name}"));
                } else {
                    m.native_missing.push((g, a, NativeMiss::New(call)));
                    super::effects::run_construct(m, e, call);
                }
            }
            _ => unknown_method(m, call, &format!("argument of unknown constructor {name}")),
        },
    }
}

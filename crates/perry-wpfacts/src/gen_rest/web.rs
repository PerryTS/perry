//! WHATWG URL and Encoding helper algorithms. The exhaustive parent match
//! routes only these variants here. Receiver operations use ordinary
//! lookup so unknown families and written methods retain conservative flows.
use super::*;
impl<'a> Gen<'a> {
    pub(super) fn web_expr(&mut self, fx: &Fx, e: &Expr) -> NodeId {
        let mi = fx.module;
        match e {
            Expr::TextEncoderNew => self.bobj(mi, "TextCodec").1,
            Expr::TextDecoderNew {
                label,
                fatal,
                ignore_bom,
            } => {
                let label = self.expr(fx, label);
                crate::builtins::string_methods::to_prim(
                    &mut self.m,
                    label,
                    "TextDecoder label DOMString",
                );
                // Dictionary getters are already explicit HIR operands;
                // booleans use ToBoolean, which never calls a hook.
                let _ = self.expr(fx, fatal);
                let _ = self.expr(fx, ignore_bom);
                self.bobj(mi, "TextCodec").1
            }
            Expr::TextEncoderEncode(src) => {
                let src = self.expr(fx, src);
                crate::builtins::string_methods::to_prim(
                    &mut self.m,
                    src,
                    "TextEncoder source USVString",
                );
                let (t, n) = self.bobj(mi, "TypedArray");
                let fa = self.m.field_any(t);
                self.m.add_prim(fa, prim::NUM);
                n
            }
            Expr::TextEncoderEncodeInto { source, dest } => {
                let src = self.expr(fx, source);
                let _ = self.expr(fx, dest);
                crate::builtins::string_methods::to_prim(
                    &mut self.m,
                    src,
                    "TextEncoder source USVString",
                );
                let (t, n) = self.bobj(mi, "Object()");
                for name in ["read", "written"] {
                    let a = self.m.atom(name);
                    let f = self.m.field(t, a);
                    self.m.add_prim(f, prim::NUM);
                }
                n
            }
            Expr::TextDecoderDecode { .. } => self.receiver_method(fx, e, "decode"),
            Expr::TextDecoderEncoding(x)
            | Expr::TextDecoderFatal(x)
            | Expr::TextDecoderIgnoreBom(x) => {
                let recv = self.expr(fx, x);
                let key = match e {
                    Expr::TextDecoderEncoding(..) => "encoding",
                    Expr::TextDecoderFatal(..) => "fatal",
                    _ => "ignoreBOM",
                };
                let a = self.m.atom(key);
                self.load(recv, Key::Named(a))
            }
            Expr::UrlInstanceToString(..) => self.receiver_method(fx, e, "toString"),
            Expr::UrlInstanceToJSON(..) => self.receiver_method(fx, e, "toJSON"),
            Expr::UrlSearchParamsGet { .. } => self.receiver_method(fx, e, "get"),
            Expr::UrlSearchParamsGetAll { .. } => self.receiver_method(fx, e, "getAll"),
            Expr::UrlSearchParamsHas { .. } => self.receiver_method(fx, e, "has"),
            Expr::UrlSearchParamsSet { .. } => self.receiver_method(fx, e, "set"),
            Expr::UrlSearchParamsAppend { .. } => self.receiver_method(fx, e, "append"),
            Expr::UrlSearchParamsDelete { .. } => self.receiver_method(fx, e, "delete"),
            Expr::UrlSearchParamsToString(..) => self.receiver_method(fx, e, "toString"),
            Expr::UrlSearchParamsSort(..) => self.receiver_method(fx, e, "sort"),
            Expr::UrlSearchParamsEntries(..) => self.receiver_method(fx, e, "entries"),
            Expr::UrlSearchParamsKeys(..) => self.receiver_method(fx, e, "keys"),
            Expr::UrlSearchParamsValues(..) => self.receiver_method(fx, e, "values"),
            Expr::UrlSearchParamsForEach { .. } => self.receiver_method(fx, e, "forEach"),
            Expr::UrlSearchParamsMissingArgs { .. } => {
                for k in kids(e) {
                    let _ = self.expr(fx, k);
                }
                self.prim_node(prim::UNDEF)
            }
            Expr::UrlGetHref(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("href");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetPathname(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("pathname");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetProtocol(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("protocol");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetHost(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("host");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetHostname(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("hostname");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetPort(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("port");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetSearch(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("search");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetHash(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("hash");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetOrigin(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("origin");
                self.load(recv, Key::Named(a))
            }
            Expr::UrlGetSearchParams(x) => {
                let recv = self.expr(fx, x);
                let a = self.m.atom("searchParams");
                let n = self.load(recv, Key::Named(a));
                let (t, _) = self.bobj(mi, "URLSearchParams");
                self.m.add_tok(n, t);
                n
            }
            Expr::UrlSetPathname { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("pathname");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetSearch { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("search");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetHash { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("hash");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetProtocol { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("protocol");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetHostname { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("hostname");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetPort { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("port");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetUsername { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("username");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetPassword { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("password");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlSetHref { url, value } => {
                let recv = self.expr(fx, url);
                let v = self.expr(fx, value);
                crate::builtins::string_methods::to_prim(&mut self.m, v, "URL setter USVString");
                let a = self.m.atom("href");
                self.store(fx, recv, Key::Named(a), v, "URL setter".into(), None, false);
                v
            }
            Expr::UrlNew { .. } | Expr::UrlSearchParamsNew(..) => {
                let args = kids(e).iter().map(|k| self.expr(fx, k)).collect();
                let (call, ret) = self.new_call(ThisArg::Undef, args, vec![]);
                crate::builtins::construct_named(
                    &mut self.m,
                    if matches!(e, Expr::UrlNew { .. }) {
                        "URL"
                    } else {
                        "URLSearchParams"
                    },
                    call,
                );
                ret
            }
            Expr::UrlPatternNew { .. } => {
                let _ = self.prim_kids(
                    fx,
                    e,
                    prim::ALL,
                    "URL constructor operand",
                    Protocol::ToPrimitive,
                );
                self.bobj(mi, "URL").1
            }
            Expr::UrlParse(..)
            | Expr::UrlParseWithBase { .. }
            | Expr::UrlCanParse(..)
            | Expr::UrlCanParseWithBase { .. } => {
                let _ = self.prim_kids(
                    fx,
                    e,
                    prim::STR,
                    "URL input/base USVString",
                    Protocol::ToPrimitive,
                );
                if matches!(e, Expr::UrlCanParse(..) | Expr::UrlCanParseWithBase { .. }) {
                    self.prim_node(prim::BOOL)
                } else {
                    let n = self.bobj(mi, "URL").1;
                    self.m.add_prim(n, prim::NULL);
                    n
                }
            }
            _ => unreachable!("web helper routing"),
        }
    }
}

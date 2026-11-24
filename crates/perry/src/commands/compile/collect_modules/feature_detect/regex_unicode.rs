//! Omit property membership only for closed, statically harmless producers.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module, ObjectPart};

fn pattern_needs_properties(pattern: &str, flags: &str) -> bool {
    flags.contains(['u', 'v'])
        || !pattern.is_ascii()
        || pattern.contains("(?<")
        || pattern.contains(r"\u")
        || pattern.contains(r"\p")
        || pattern.contains(r"\P")
}

fn producer_key(key: &str) -> bool {
    matches!(
        key,
        "RegExp"
            | "constructor"
            | "compile"
            | "URLPattern"
            | "prototype"
            | "String"
            | "Object"
            | "Function"
            | "call"
            | "apply"
            | "bind"
    )
}

fn consumer_key(key: &str) -> bool {
    matches!(key, "split" | "test" | "exec")
}

fn coercion_needs_properties(module: &Module, pattern: &Expr) -> bool {
    let conversion_key = |key: &str| matches!(key, "toString" | "valueOf" | "__proto__");
    match pattern {
        Expr::String(pattern) => pattern_needs_properties(pattern, ""),
        Expr::Number(_) | Expr::Integer(_) | Expr::Bool(_) | Expr::Null | Expr::Undefined => false,
        // A fresh ordinary object's default conversion is "[object Object]".
        // Field values do not participate. Prototype exposure is a separate
        // conservative guard; spreads and computed keys cannot use this proof.
        Expr::Object(fields) => fields.iter().any(|(key, _)| conversion_key(key)),
        Expr::New { class_name, .. } => !module.classes.iter().any(|class| {
            class.name == *class_name
                && class.is_literal_shape()
                && class.computed_members.is_empty()
                && class
                    .fields
                    .iter()
                    .all(|field| !conversion_key(&field.name))
        }),
        _ => true,
    }
}

fn expression(module: &Module, expr: &Expr) -> bool {
    match expr {
        // Lowering represents the built-in Map constructor dynamically even
        // with no input. It cannot supply a regex; global replacement and
        // constructor/prototype exposure are separate capability guards.
        Expr::NewDynamic { callee, args, .. }
            if args.is_empty()
                && matches!(callee.as_ref(), Expr::PropertyGet { object, property, .. }
                    if matches!(object.as_ref(), Expr::GlobalGet(0)) && property == "Map") =>
        {
            return false
        }
        Expr::StringSplit(subject, delimiter) => {
            if let Expr::RegExp { pattern, flags } = delimiter.as_ref() {
                return pattern_needs_properties(pattern, flags) || expression(module, subject);
            }
        }
        Expr::Call { callee, args, .. } | Expr::RegionCall { callee, args, .. } => {
            if matches!(
                callee.as_ref(),
                Expr::FuncRef(_) | Expr::ExternFuncRef { .. }
            ) {
                return args.iter().any(|e| expression(module, e));
            }
            // A computed or extracted callable can be RegExp or compile,
            // regardless of the key's TypeScript annotation.
            if !matches!(callee.as_ref(), Expr::PropertyGet { .. }) {
                return true;
            }
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if matches!(property.as_str(), "search" | "match" | "matchAll") {
                    return args
                        .first()
                        .is_some_and(|pattern| coercion_needs_properties(module, pattern))
                        || expression(module, object)
                        || args.iter().any(|e| expression(module, e));
                }
                if property == "split" {
                    if let Some(Expr::RegExp { pattern, flags }) = args.first() {
                        return pattern_needs_properties(pattern, flags)
                            || expression(module, object)
                            || args[1..].iter().any(|e| expression(module, e));
                    }
                }
                if matches!(property.as_str(), "test" | "exec") {
                    if let Expr::RegExp { pattern, flags } = object.as_ref() {
                        return pattern_needs_properties(pattern, flags)
                            || args.iter().any(|e| expression(module, e));
                    }
                }
                if consumer_key(property) {
                    return expression(module, object)
                        || args.iter().any(|e| expression(module, e));
                }
            }
        }
        // A retained regex value exposes its constructor and compile method,
        // even through a computed key whose annotation claims to be numeric.
        Expr::RegExp { .. } => return true,
        // A glob's POSIX classes (`[[:alpha:]]`) compile to Unicode property classes.
        Expr::PathMatchesGlob(..)
        | Expr::PathWin32 {
            method: perry_hir::PathWin32Method::MatchesGlob,
            ..
        } => return true,
        Expr::ObjectGetPrototypeOf(_)
        | Expr::ReflectGetPrototypeOf(_)
        | Expr::ObjectSetPrototypeOf(_, _)
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ObjectDefineProperty(_, _, _)
        | Expr::ObjectDefineProperties(_, _)
        | Expr::ReflectDefineProperty { .. }
        | Expr::ReflectSet { .. }
        | Expr::ObjectAssign { .. }
        | Expr::ObjectCreate(_, _)
        | Expr::ProxyNew { .. }
        | Expr::CallSpread { .. }
        | Expr::NewDynamic { .. }
        | Expr::NewDynamicSpread { .. }
        | Expr::ReflectApply { .. }
        | Expr::ReflectConstruct { .. }
        | Expr::Closure { .. }
        | Expr::FuncRef(_)
        | Expr::ExternFuncRef { .. } => return true,
        // Coercion, custom species and RegExp.prototype.compile can obtain a
        // pattern or flags from arbitrary user code. Do not infer string values
        // from annotations or a local binding's initializer.
        Expr::RegExpDynamic { .. } | Expr::StringMatch { .. } | Expr::StringMatchAll { .. } => {
            return true;
        }
        Expr::PropertyGet { property, .. }
            if producer_key(property)
                || consumer_key(property)
                || matches!(property.as_str(), "search" | "match" | "matchAll") =>
        {
            return true
        }
        Expr::PropertySet { property, .. } | Expr::LiteralSlotInit { key: property, .. }
            if consumer_key(property) =>
        {
            return true
        }
        Expr::Object(properties) if properties.iter().any(|(key, _)| consumer_key(key)) => {
            return true
        }
        // A computed key may evaluate to a consumer key, so only a literal
        // string key is proven absent.
        Expr::ObjectSpread { parts }
            if parts.iter().any(|part| match part {
                ObjectPart::Spread(_) => false,
                ObjectPart::Define(key, _) => consumer_key(key),
                ObjectPart::Computed { key, .. } => {
                    !matches!(key.as_ref(), Expr::String(key) if !consumer_key(key))
                }
            }) =>
        {
            return true
        }
        Expr::IndexGet { index, .. }
        | Expr::RegionOut { index, .. }
        | Expr::ReflectGet { key: index, .. }
        | Expr::ReflectGetOwnPropertyDescriptor { key: index, .. }
        | Expr::ObjectGetOwnPropertyDescriptor(_, index)
            if matches!(index.as_ref(), Expr::String(key) if producer_key(key)) =>
        {
            return true
        }
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= expression(module, child));
    found
}

pub(super) fn needed(module: &Module) -> bool {
    // An opaque read of a data container cannot create a RegExp. Keep the
    // capabilities that can supply one: global/constructor access, escaping
    // regexes, native producers, and user-defined consumers of a literal.
    // The generic computed-member detector also covers non-regex APIs.
    // An inherited RegExp constructor accepts patterns without a dedicated
    // RegExpDynamic expression at its `new Subclass(...)` call sites.
    module.classes.iter().any(|class| {
        class.native_extends.is_some()
            || class.extends_name.is_some()
            || class.extends_expr.is_some()
            || class.extends.is_some()
            || !class.computed_members.is_empty()
            || class
                .methods
                .iter()
                .chain(&class.static_methods)
                .any(|method| consumer_key(&method.name))
            || class
                .fields
                .iter()
                .chain(&class.static_fields)
                .any(|field| consumer_key(&field.name))
    }) || super::global_object::may_escape(module)
        || super::module_any_expr(module, &mut |e| {
            let needed = expression(module, e);
            if needed {
                log::debug!("regex Unicode producer in {}: {e:?}", module.name);
            }
            needed
        })
}

#[cfg(test)]
mod tests {
    use super::super::tests::detect_for_source;

    #[test]
    fn regex_unicode_selection_covers_static_and_dynamic_producers() {
        for source in [
            r"/\p{L}/u.test('é');",
            r"/[[a-z]--[q]]/v.test('a');",
            r"import * as path from 'node:path'; path.matchesGlob('a', process.argv[2]);",
            r"import * as path from 'node:path'; path.win32.matchesGlob('a', process.argv[2]);",
            r"/(?<α>a)/.test('a');",
            r"/(?<\u03b1>a)/.test('a');",
            r"/k/iu.test('K');",
            r"new RegExp(process.argv[2], 'u');",
            r"RegExp(process.argv[2]);",
            r"const r = /a/; function f(k: number) { return r[k]('\\p{L}', 'u'); } f('compile' as any);",
            r"function expose() { return /a/; }",
            r"const o = { split(r) { return r; } }; o.split(/a/);",
            r"const o = {}; Object.defineProperty(o, 'split', { value(r) { return r; } }); o.split(/a/);",
            r"const o = {}; const key = process.argv[2]; o[key] = r => r; o.split(/a/);",
            r"class C { helper(r) { return r; } } const c = new C(); c[process.argv[2]] = c.helper; const r = c.split(/a/); function f(k: number) { r[k]('\\p{L}', 'u'); } f('compile' as any);",
            r"class C { helper(r) { return r; } } const c = new C(); c[process.argv[2]] = c.helper; const r = c.split(/a/); const f = r[process.argv[3]]; f('\\p{L}', 'u');",
            r"class C { helper(r) { return r; } } const c = new C(); c[process.argv[2]] = c.helper; const r = c.split(/a/); const f = r[process.argv[3]]; f.apply(r, ['\\p{L}', 'u']);",
            r"class C { split(r) { return r; } } new C().split(/a/);",
            r"const list = [/a/];",
            r"String.prototype.split = function(r) { return []; }; 'x'.split(/a/);",
            r"class R extends RegExp {} new R(process.argv[2], 'u');",
            r"const C = RegExp; new C(process.argv[2], 'u');",
            r"const r = /a/; r.compile(process.argv[2], 'u');",
            r"const r = /a/; r[process.argv[2]]('\\p{L}', 'u');",
            r"const r = /a/; new r.constructor('\\p{L}', 'u');",
            r"'a'.match(process.argv[2]);",
            r"'a'.search({ toString() { return process.argv[2]; } });",
            r"const g = globalThis; new g[process.argv[2]]('\\p{L}', 'u');",
            r"class C { static r = /\p{L}/u; f() { return /(?<α>a)/; } }",
            r"function f(r = /\p{L}/u) { return r; }",
        ] {
            assert!(detect_for_source(source).uses_regex_unicode, "{source}");
        }
    }

    #[test]
    fn regex_unicode_selection_omits_closed_ascii_literals() {
        for source in [
            r"'a b'.split(/\s+/);",
            r"/^[a-z]+$/i.test('hello');",
            r"class C { search() { return 1; } } const c = new C(); c.search();",
            r"class C { x = 1; search(options) { return options.maxNodes; } } const c = new C(); c.search({level: 10, maxNodes: Number(process.argv[2])});",
            r"const a = [1, 2]; console.log(a[1]);",
            r"function f(values: any, key: any) { return values[key]; } console.log(f([1, 2], 1)); 'a b'.split(/\s+/);",
            r"class C { table = new Map<string, number>(); parse(fen: string) { return fen.trim().split(/\s+/); } } new C().parse('a b');",
            r"console.log('hello');",
        ] {
            assert!(!detect_for_source(source).uses_regex_unicode, "{source}");
        }
    }
}

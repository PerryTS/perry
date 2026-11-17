//! Omit property membership only for closed, statically harmless producers.
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Module};

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
    )
}

fn consumer_key(key: &str) -> bool {
    matches!(key, "split" | "test" | "exec")
}

fn expression(expr: &Expr) -> bool {
    match expr {
        Expr::StringSplit(subject, delimiter) => {
            if let Expr::RegExp { pattern, flags } = delimiter.as_ref() {
                return pattern_needs_properties(pattern, flags) || expression(subject);
            }
        }
        Expr::Call { callee, args, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if property == "split" {
                    if let Some(Expr::RegExp { pattern, flags }) = args.first() {
                        return pattern_needs_properties(pattern, flags)
                            || expression(object)
                            || args[1..].iter().any(expression);
                    }
                }
                if matches!(property.as_str(), "test" | "exec") {
                    if let Expr::RegExp { pattern, flags } = object.as_ref() {
                        return pattern_needs_properties(pattern, flags)
                            || args.iter().any(expression);
                    }
                }
            }
        }
        // A retained regex value exposes its constructor and compile method,
        // even through a computed key whose annotation claims to be numeric.
        Expr::RegExp { .. } => return true,
        Expr::ObjectGetPrototypeOf(_) | Expr::ReflectGetPrototypeOf(_) => return true,
        // Coercion, custom species and RegExp.prototype.compile can obtain a
        // pattern or flags from arbitrary user code. Do not infer string values
        // from annotations or a local binding's initializer.
        Expr::RegExpDynamic { .. } | Expr::StringMatch { .. } | Expr::StringMatchAll { .. } => {
            return true;
        }
        Expr::PropertyGet { property, .. } if producer_key(property) => return true,
        Expr::PropertySet { property, .. } | Expr::LiteralSlotInit { key: property, .. }
            if consumer_key(property) =>
        {
            return true
        }
        Expr::Object(properties) if properties.iter().any(|(key, _)| consumer_key(key)) => {
            return true
        }
        Expr::ObjectSpread { parts }
            if parts
                .iter()
                .any(|(key, _)| key.as_deref().is_some_and(consumer_key)) =>
        {
            return true
        }
        Expr::IndexGet { index, .. }
        | Expr::ReflectGet { key: index, .. }
        | Expr::ReflectGetOwnPropertyDescriptor { key: index, .. }
        | Expr::ObjectGetOwnPropertyDescriptor(_, index)
            if matches!(index.as_ref(), Expr::String(key) if producer_key(key)) =>
        {
            return true
        }
        Expr::Closure { body, .. } if super::stmts_any_expr(body, &mut expression) => return true,
        _ => {}
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= expression(child));
    found
}

pub(super) fn needed(module: &Module) -> bool {
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
        || super::regex_members::needed(module)
        || super::module_any_expr(module, &mut expression)
}

#[cfg(test)]
mod tests {
    use super::super::tests::detect_for_source;

    #[test]
    fn regex_unicode_selection_covers_static_and_dynamic_producers() {
        for source in [
            r"/\p{L}/u.test('é');",
            r"/[[a-z]--[q]]/v.test('a');",
            r"/(?<α>a)/.test('a');",
            r"/(?<\u03b1>a)/.test('a');",
            r"/k/iu.test('K');",
            r"new RegExp(process.argv[2], 'u');",
            r"RegExp(process.argv[2]);",
            r"const r = /a/; function f(k: number) { return r[k]('\\p{L}', 'u'); } f('compile' as any);",
            r"function expose() { return /a/; }",
            r"const o = { split(r) { return r; } }; o.split(/a/);",
            r"class C { split(r) { return r; } } new C().split(/a/);",
            r"const list = [/a/];",
            r"String.prototype.split = function(r) { return []; }; 'x'.split(/a/);",
            r"class R extends RegExp {} new R(process.argv[2], 'u');",
            r"const C = RegExp; new C(process.argv[2], 'u');",
            r"const r = /a/; r.compile(process.argv[2], 'u');",
            r"const r = /a/; r[process.argv[2]]('\\p{L}', 'u');",
            r"const r = /a/; new r.constructor('\\p{L}', 'u');",
            r"'a'.match(process.argv[2]);",
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
            r"const a = [1, 2]; console.log(a[1]);",
            r"console.log('hello');",
        ] {
            assert!(!detect_for_source(source).uses_regex_unicode, "{source}");
        }
    }
}

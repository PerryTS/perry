//! #9486: display-name registration for function BODY and method symbols,
//! split from `artifacts.rs` to keep `emit_module_artifacts` under the
//! 2000-line file cap. See the comments inside for why body symbols and
//! class-method symbols are registered in addition to the wrapper keys.

use std::collections::HashMap;

use crate::module::LlModule;
use perry_hir::Module as HirModule;

pub(super) fn extend_body_symbol_display_names(
    hir: &HirModule,
    func_names: &HashMap<u32, String>,
    method_names: &HashMap<(String, String), String>,
    llmod: &LlModule,
    user_fn_display_names: &mut Vec<(String, String)>,
) {
    // #9486: the same name against the function BODY symbol as well.
    //
    // The wrapper address above is what a closure VALUE carries, so it is what
    // `fn.name` needs — but it is not what a return address on the native
    // stack points into. A direct call from one compiled function to another
    // targets `perry_fn_<prefix>__<name>` itself, so an `Error.stack` frame
    // resolves against the body or against nothing at all. Both keys map to
    // the same name, and `fn.name` still reads the wrapper key it always did,
    // so nothing that consulted this registry before sees a different answer.
    let body_symbol_display_names: Vec<(String, String)> = hir
        .functions
        .iter()
        .filter_map(|f| {
            let display = hir.closure_display_names.get(&f.id).cloned().or_else(|| {
                if f.name.is_empty() || f.name.starts_with('_') {
                    None
                } else {
                    Some(f.name.clone())
                }
            })?;
            func_names
                .get(&f.id)
                .filter(|sym| llmod.has_function(sym))
                .map(|sym| (sym.clone(), display))
        })
        .collect();
    user_fn_display_names.extend(body_symbol_display_names);
    // #9486: class methods, under the `Class.method` label node uses for a
    // prototype-method frame. `method_names` is the map codegen itself keyed
    // the emitted `perry_method_*` symbols by, and the `__perry_wrap_*`
    // generator earlier in this function walks exactly this pair of loops
    // with the same `.get(...)` guard — so every symbol here is one this module
    // definitely emitted, which is the condition the #318/#343 "use of
    // undefined value" class turns on. Only the BODY symbol is registered:
    // the wrapper address is what `fn.name` reads, and giving a method a
    // `.name` it never had is a separate, observable change.
    {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        // `method_names` is a DISPATCH registry, not an emission record: it
        // carries entries this module never defines a body for (an accessor
        // reached only through a cross-module or typed path, a stale key from
        // a shape that lowered elsewhere). Registering one of those emits a
        // `js_register_function_name_static(ptr @perry_method_…)` against a symbol
        // that does not exist, and the module fails to build with "reference
        // to unknown global" — measured on the claude-code bundle, where
        // exactly one getter (`UT7.__get_get_extensionName`) had a registry
        // key and no definition out of ~46k functions. `has_function` is the
        // authority on what this module actually emitted, so every name below
        // goes through it.
        let mut push_defined = |symbol: String, display: String| {
            if symbol.is_empty() || display.is_empty() || !llmod.has_function(&symbol) {
                return;
            }
            if seen.insert(symbol.clone()) {
                user_fn_display_names.push((symbol, display));
            }
        };
        for class in &hir.classes {
            for method in &class.methods {
                let Some(symbol) = method_names
                    .get(&(class.name.clone(), method.name.clone()))
                    .cloned()
                else {
                    continue;
                };
                push_defined(symbol, format!("{}.{}", class.name, method.name));
            }
            for method in &class.static_methods {
                let Some(symbol) = method_names
                    .get(&(class.name.clone(), method.name.clone()))
                    .cloned()
                else {
                    continue;
                };
                push_defined(symbol, format!("{}.{}", class.name, method.name));
            }
            // Accessors are keyed with the `__get_` / `__set_` prefix
            // `method_registry` gives them, and node labels their frames
            // `get x` / `set x`.
            for (accessors, prefix, label) in [
                (&class.getters, "__get_", "get"),
                (&class.setters, "__set_", "set"),
            ] {
                for (prop, _) in accessors {
                    let Some(symbol) = method_names
                        .get(&(class.name.clone(), format!("{prefix}{prop}")))
                        .cloned()
                    else {
                        continue;
                    };
                    push_defined(symbol, format!("{label} {prop}"));
                }
            }
            // The constructor is registered in `method_names` under the
            // synthesized `<Class>_constructor` method name (method_registry.rs
            // emits one for EVERY class, explicit or not), and node labels its
            // frame `new <Class>`.
            //
            // Registering these is not only about naming THEIR frames. A
            // registry entry names a function START and carries no end, so an
            // address inside an UNREGISTERED function resolves to whatever
            // registered function precedes it — measured: a `new Widget()`
            // frame came out labelled `main`. Every emitted function this list
            // covers is one that can no longer borrow a neighbour's name.
            let ctor_key = (class.name.clone(), format!("{}_constructor", class.name));
            if let Some(symbol) = method_names.get(&ctor_key).cloned() {
                push_defined(symbol, format!("new {}", class.name));
            }
        }
    }
}

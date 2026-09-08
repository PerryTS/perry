use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Export, Expr, Function, Module, Param, Stmt};

fn empty_opts() -> CompileOptions {
    CompileOptions {
        emit_ir_only: true,
        output_type: "executable".into(),
        ..Default::default()
    }
}

fn test_function(id: u32, value: f64) -> Function {
    Function {
        id,
        name: "lex".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Number,
        body: vec![Stmt::Return(Some(Expr::Number(value)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        was_plain_async: false,
        was_unrolled: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
    }
}

#[test]
fn dollar_export_aliases_resolve_function_identity_not_sanitized_spelling() {
    let mut module = Module::new("export_collision.js");
    for (id, name, arity) in [(1, "$n", 0), (2, "_n", 1), (3, "$wide", 6), (4, "_wide", 0)] {
        let mut function = test_function(id, id as f64);
        function.name = name.into();
        function.is_exported = true;
        function.params = (0..arity)
            .map(|i| Param {
                id: id * 100 + i,
                name: format!("a{i}"),
                ty: Type::Any,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            })
            .collect();
        module.functions.push(function);
        module.exports.push(Export::Named {
            local: name.into(),
            exported: name.into(),
        });
        module.exported_functions.push((name.into(), id));
    }
    // An exported function alias can carry a local name different from the
    // original body. The recorded function ID is authoritative in that case.
    module.exports.push(Export::Named {
        local: "alias".into(),
        exported: "$renamedWide".into(),
    });
    module.exported_functions.push(("$renamedWide".into(), 3));
    let ir = String::from_utf8(compile_module(&module, empty_opts()).unwrap()).unwrap();
    for (exported, body, arity) in [
        ("$n", "$n", 0),
        ("$wide", "$wide", 6),
        ("$renamedWide", "$wide", 6),
    ] {
        let target = perry_codegen::user_function_symbol(&module.name, body);
        for wrapper in [false, true] {
            let prefix = if wrapper { "__perry_wrap_" } else { "" };
            let symbol = format!("{prefix}perry_fn_export_collision_js__{exported}");
            let definition = ir
                .split(&format!("define double @{symbol}("))
                .nth(1)
                .unwrap_or_else(|| panic!("missing {symbol}"))
                .split("\n}")
                .next()
                .unwrap();
            let mut args = if wrapper {
                vec!["i64 %this_closure".to_string()]
            } else {
                Vec::new()
            };
            args.extend((0..arity).map(|i| format!("double %a{i}")));
            let call = format!("call double @{prefix}{target}({})", args.join(", "));
            assert!(
                definition.contains(&call),
                "{symbol} must forward to {call}, got:\n{definition}"
            );
        }
    }
}

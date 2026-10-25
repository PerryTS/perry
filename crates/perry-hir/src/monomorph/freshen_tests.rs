use super::*;

fn lowered(src: &str) -> Module {
    let ast = perry_parser::parse_typescript(src, "identity.ts").unwrap();
    let mut module = crate::lower_module(&ast, "identity", "identity.ts").unwrap();
    crate::monomorphize_module(&mut module);
    module
}

#[test]
fn copied_functions_own_their_parameters_and_catches() {
    let module = lowered(
        r#"
        function make<T>(value: T) {
            let current = value;
            try { throw "caught"; } catch (error) { current = value; }
            return (extra: number = 1) => { current = value; return [current, extra]; };
        }
        const a = make<number>(1);
        const b = make<string>("b");
        console.log(a(), b());
    "#,
    );
    let bodies: Vec<_> = module
        .functions
        .iter()
        .filter(|f| f.name.starts_with("make"))
        .collect();
    assert!(
        bodies.len() >= 3,
        "a template and two specializations must be live"
    );
    let mut locals = BTreeSet::new();
    let mut funcs = BTreeSet::new();
    for f in bodies {
        let mut owned = Owned::default();
        owned.function_frame(f);
        assert!(
            owned.locals.len() >= 3,
            "parameters, let and catch belong to this frame"
        );
        for id in owned.locals {
            assert!(locals.insert(id), "two bodies own local {id}");
        }
        for id in owned.funcs {
            assert!(funcs.insert(id), "two bodies own closure {id}");
        }
    }
}

#[test]
fn copied_classes_have_distinct_method_and_constructor_frames() {
    let module = lowered(
        r#"
        class Box<T> {
            value: T;
            constructor(value: T) { this.value = value; }
            get() { const result = this.value; return result; }
            get item() { return this.value; }
            set item(value: T) { this.value = value; }
        }
        const a = new Box<number>(1);
        const b = new Box<string>("b");
        console.log(a.get(), b.get());
    "#,
    );
    let mut locals = BTreeSet::new();
    let mut funcs = BTreeSet::new();
    let bodies: Vec<_> = module
        .classes
        .iter()
        .filter(|c| c.name.starts_with("Box"))
        .collect();
    assert!(bodies.len() >= 3);
    for c in bodies {
        let mut owned = Owned::default();
        owned.class_frame(c);
        for id in owned.locals {
            assert!(locals.insert(id), "two classes own local {id}");
        }
        for id in owned.funcs {
            assert!(funcs.insert(id), "two classes own function {id}");
        }
    }
}

#[test]
fn mapped_arguments_follow_the_specialized_formal_parameter() {
    let module = lowered(
        r#"
        function read<T>(value: T) { arguments[0] = value; return value; }
        console.log(read<number>(1), read<string>("b"));
    "#,
    );
    let bodies: Vec<_> = module
        .functions
        .iter()
        .filter(|f| f.name.starts_with("read$"))
        .collect();
    assert!(bodies.len() >= 2);
    for f in bodies {
        let owned: BTreeSet<_> = f.params.iter().map(|p| p.id).collect();
        let mapped: Vec<_> = f
            .params
            .iter()
            .filter_map(|p| p.arguments_object.as_ref())
            .flat_map(|m| &m.mapped_parameter_ids)
            .collect();
        assert!(
            !mapped.is_empty(),
            "sloppy arguments must map at least one formal"
        );
        for (_, id) in mapped {
            assert!(
                owned.contains(id),
                "mapped arguments points outside its own frame"
            );
        }
    }
}

#[test]
fn copied_functions_keep_their_name_and_async_generator_metadata() {
    let ast = perry_parser::parse_typescript(
        "function make<T>(value: T) { return function named() { return value; }; } console.log(make<number>(1));",
        "metadata.ts",
    ).unwrap();
    let mut module = crate::lower_module(&ast, "metadata", "metadata.ts").unwrap();
    let original = module.functions.iter().find(|f| f.name == "make").unwrap();
    let root = original.id;
    let mut owned = Owned::default();
    owned.function(original);
    let closure = *owned.funcs.first().expect("named closure");
    for id in [root, closure] {
        module.async_step_closures.insert(id);
        module.async_generator_funcs.insert(id);
        module.gen_param_prologue_len.insert(id, 2);
        module.closure_source_text.insert(
            id,
            crate::ir::FunctionSourceMetadata {
                text: "function retained() {}".into(),
                is_non_strict_ordinary: true,
            },
        );
    }
    module.closure_display_names.insert(root, "make".into());
    crate::monomorphize_module(&mut module);
    let copy = module
        .functions
        .iter()
        .find(|f| f.name.starts_with("make$"))
        .unwrap();
    let mut owned = Owned::default();
    owned.function(copy);
    let copied_closure = *owned.funcs.first().unwrap();
    for id in [copy.id, copied_closure] {
        assert!(module.async_step_closures.contains(&id));
        assert!(module.async_generator_funcs.contains(&id));
        assert_eq!(module.gen_param_prologue_len.get(&id), Some(&2));
        assert!(module.closure_source_text.contains_key(&id));
    }
    assert_eq!(
        module
            .closure_display_names
            .get(&copy.id)
            .map(String::as_str),
        Some("make")
    );
    assert_eq!(
        module
            .closure_display_names
            .get(&copied_closure)
            .map(String::as_str),
        Some("named")
    );
}

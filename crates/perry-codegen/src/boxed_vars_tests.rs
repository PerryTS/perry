    use super::*;
    use perry_hir::types::Type;
    use perry_hir::{Expr, Stmt};

    fn closure(func_id: u32, captured_id: u32) -> Expr {
        Expr::Closure {
            func_id,
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::LocalGet(captured_id)))],
            captures: vec![captured_id],
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: false,
        }
    }

    #[test]
    fn generator_confined_captures_are_slots_and_shared_captures_are_cells() {
        let mut state = closure(10, 1);
        if let Expr::Closure { body, .. } = &mut state {
            body.insert(
                0,
                Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Number(2.0)))),
            );
        }
        let mut stmts = vec![
            Stmt::Let {
                id: 1,
                name: "state".into(),
                ty: Type::Number,
                mutable: true,
                init: Some(Expr::Number(0.0)),
            },
            Stmt::Return(Some(Expr::LinkGeneratorPrototype {
                obj: Box::new(state),
                is_async: false,
                fixed_state: true,
                i32_controls: vec![1],
                i1_controls: vec![],
            })),
        ];
        assert!(!collect_boxed_vars(&stmts).contains(&1));
        let mut numbers = HashSet::new();
        let mut booleans = HashSet::new();
        collect_compiler_private_async_control_locals_in_stmts(&stmts, &mut numbers, &mut booleans);
        assert_eq!(numbers, HashSet::from([1]));
        assert!(booleans.is_empty());
        let user = Stmt::Let {
            id: 20,
            name: "__gen_state".into(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Number(1.0)),
        };
        collect_compiler_private_async_control_locals_in_stmts(
            &[user],
            &mut numbers,
            &mut booleans,
        );
        assert_eq!(
            numbers,
            HashSet::from([1]),
            "user spelling is not a scalar-slot proof"
        );
        stmts.insert(0, Stmt::PreallocateBoxes(vec![1]));
        assert!(collect_boxed_vars(&stmts).contains(&1));
    }

    #[test]
    fn captured_closure_value_is_boxed_only_for_real_self_capture() {
        // #10520: the outer closure references `f`, but `f`'s initializer
        // does not. Only `self_ref` captures itself before initialization.
        let stmts = vec![
            Stmt::Let {
                id: 1,
                name: "f".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure(10, 3)),
            },
            Stmt::Let {
                id: 2,
                name: "self_ref".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure(11, 2)),
            },
            Stmt::Expr(closure(12, 1)),
        ];
        let boxed = collect_boxed_vars(&stmts);
        assert!(!boxed.contains(&1), "captured const closure needs no cell");
        assert!(
            boxed.contains(&2),
            "a self-capturing initializer needs a cell"
        );
    }

    #[test]
    fn simple_refinement_uses_shared_hir_inference_for_constructed_values() {
        assert_eq!(
            refine_type_from_init_simple(&Expr::DateNew(vec![])),
            Some(Type::Named("Date".to_string()))
        );
        assert_eq!(
            refine_type_from_init_simple(&Expr::Uint8ArrayNew(None)),
            Some(Type::Named("Uint8Array".to_string()))
        );
        assert_eq!(
            refine_type_from_init_simple(&Expr::CryptoRandomBytes(Box::new(Expr::Integer(8)))),
            Some(Type::Named("Uint8Array".to_string()))
        );
        assert_eq!(
            refine_type_from_init_simple(&Expr::ArraySlice {
                layout: None,
                array: Box::new(Expr::Array(vec![], None)),
                start: Box::new(Expr::Integer(0)),
                end: None,
            }),
            Some(Type::Array(Box::new(Type::Any)))
        );
        assert_eq!(
            refine_type_from_init_simple(&Expr::ArrayJoin {
                array: Box::new(Expr::Array(vec![], None)),
                separator: None,
            }),
            Some(Type::String)
        );
    }

    #[test]
    fn simple_refinement_filters_unknown_and_void_results() {
        assert_eq!(refine_type_from_init_simple(&Expr::LocalGet(404)), None);
        assert_eq!(refine_type_from_init_simple(&Expr::Undefined), None);
        assert_eq!(refine_type_from_init_simple(&Expr::ProcessExit(None)), None);
    }

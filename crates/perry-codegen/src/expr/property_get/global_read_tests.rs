//! A global identifier and a reified static use ordinary property sites.
use super::*;

#[test]
fn global_reads_and_static_values_use_shape_sites() {
    for name in ["Array", "Date", "Object", "Number", "WeakMap"] {
        let mut module = Module::new("global_read.ts");
        module.init.push(Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: name.to_string(),
            byte_offset: 0,
        }));
        let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap()).unwrap();
        assert!(ir.contains("call double @js_get_global_this("), "{ir}");
        assert!(
            ir.contains("pic.hit"),
            "a global needs the ordinary inline slot arm: {ir}"
        );
        assert!(
            !ir.contains("call double @js_get_global_this_builtin_value("),
            "{ir}"
        );
    }
    let mut module = Module::new("global_static_read.ts");
    module.init.push(Stmt::Expr(Expr::PropertyGet {
        object: Box::new(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: "Object".to_string(),
            byte_offset: 0,
        }),
        property: "hasOwn".to_string(),
        byte_offset: 0,
    }));
    let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap()).unwrap();
    assert!(
        ir.matches("pic.hit").count() >= 2,
        "both receiver and static must use read sites: {ir}"
    );
    assert!(
        !ir.contains("call double @js_get_global_this_builtin_value("),
        "{ir}"
    );
    assert!(
        !ir.contains("call double @js_object_get_field_by_name_f64("),
        "{ir}"
    );
}

#[test]
fn global_object_properties_do_not_select_a_builtin_by_member_name() {
    for property in ["isBuffer", "isView", "hasOwn", "fromBase64", "supports"] {
        let mut module = Module::new("global_member.ts");
        module.init.push(Stmt::Expr(Expr::PropertyGet {
            object: Box::new(Expr::GlobalGet(0)),
            property: property.to_string(),
            byte_offset: 0,
        }));
        let ir = String::from_utf8(compile_module(&module, ir_opts(false, None)).unwrap()).unwrap();
        let sites = ir
            .lines()
            .filter(|line| {
                line.starts_with("@perry_ic_") && line.ends_with("= private global ptr null")
            })
            .count();
        assert_eq!(
            sites, 1,
            "globalThis.{property} needs only its own property site: {ir}"
        );
    }
}

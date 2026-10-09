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
        object: Box::new(Expr::GlobalGet(0)),
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

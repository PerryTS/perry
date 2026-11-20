//! #10476: verdict tests for builtin-named method calls in HIR lowering.
//!
//! A Date / Array method NAME is not proof of the receiver's kind: dayjs's
//! `Dayjs.prototype.toISOString`, a class's `getTime()` or `toSorted()`, an
//! object literal's `setHours`. These assert which lowering a call got — the
//! builtin intrinsic only for a statically proven receiver, a generic method
//! call otherwise (codegen then checks the runtime kind).

#![cfg(test)]

use crate::Module;
use perry_diagnostics::SourceCache;

fn lower(src: &str) -> String {
    let src = src.to_string();
    let module: Module = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "builtin_named_user_methods.ts",
                &mut cache,
            )
            .expect("parse should succeed");
            crate::lower_module(&parsed.module, "test", "builtin_named_user_methods.ts")
                .expect("lower should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked");
    format!("{module:?}")
}

#[test]
fn date_names_on_unproven_receivers_stay_method_calls() {
    let hir = lower(
        r#"
        function Dayjs(this: any, ms: number) { this.$ms = ms; }
        Dayjs.prototype.toISOString = function () { return "x"; };
        Dayjs.prototype.toJSON = function (this: any) { return this.toISOString(); };
        class Clock { getTime() { return 1; } setHours(h: number) { return h; } }
        const c = new Clock();
        const lit = { getFullYear() { return 2; }, toLocaleString() { return "l"; } };
        function viaAny(x: any) {
            return [x.getTime(), x.setUTCHours(1), x.toDateString(), x.toLocaleTimeString()];
        }
        console.log(c.getTime(), c.setHours(3), lit.getFullYear(), lit.toLocaleString(), viaAny(c));
        "#,
    );
    for intrinsic in [
        "DateToISOString",
        "DateGetTime",
        "DateSetHours",
        "DateGetFullYear",
        "DateToLocaleString",
        "DateSetUtcHours",
        "DateToDateString",
        "DateToLocaleTimeString",
    ] {
        assert!(
            !hir.contains(intrinsic),
            "an unproven receiver was lowered to the Date intrinsic {intrinsic}: {hir}"
        );
    }
}

#[test]
fn proven_date_receivers_keep_the_date_intrinsics() {
    let hir = lower(
        r#"
        const d = new Date(0);
        function takesDate(p: Date) { return p.getUTCHours(); }
        console.log(
            d.getTime(), d.toISOString(), d.setHours(1), new Date(5).getFullYear(),
            takesDate(d), d.toLocaleString(), (12345).toLocaleString(),
        );
        "#,
    );
    for intrinsic in [
        "DateGetTime",
        "DateToISOString",
        "DateSetHours",
        "DateGetFullYear",
        "DateGetUtcHours",
        "DateToLocaleString",
    ] {
        assert!(
            hir.contains(intrinsic),
            "a proven Date receiver lost the {intrinsic} intrinsic: {hir}"
        );
    }
}

#[test]
fn array_copy_names_on_unproven_receivers_stay_method_calls() {
    let hir = lower(
        r#"
        const add = (a: number, b: number) => a + b;
        class Vec { toSorted() { return this; } toReversed() { return this; } reduceRight(f: any) { return f; } }
        const v = new Vec();
        const lit: any = { flat() { return 1; }, flatMap(f: any) { return f; }, toSpliced(i: number) { return i; } };
        const proto: any = Object.create({ toSorted() { return 0; }, copyWithin(a: number, b: number) { return a + b; } });
        function viaAny(x: any) { return [x.toSorted(), x.toReversed(), x.reduceRight(add)]; }
        console.log(
            v.toSorted(), v.toReversed(), v.reduceRight(add), lit.flat(), lit.flatMap(add),
            lit.toSpliced(1), proto.toSorted(), proto.copyWithin(0, 1), viaAny(v),
        );
        "#,
    );
    for intrinsic in [
        "ArrayToSorted",
        "ArrayToReversed",
        "ArrayReduceRight",
        "ArrayFlat {",
        "ArrayFlatMap",
        "ArrayToSpliced",
        "ArrayCopyWithin",
    ] {
        assert!(
            !hir.contains(intrinsic),
            "an unproven receiver was lowered to the Array intrinsic {intrinsic}: {hir}"
        );
    }
}

#[test]
fn proven_array_receivers_keep_the_array_intrinsics() {
    let hir = lower(
        r#"
        const add = (a: number, b: number) => a + b;
        const arr: number[] = [3, 1, 2];
        const nested = [1, [2]];
        console.log(
            arr.toSorted(), arr.toReversed(), arr.reduceRight(add), nested.flat(),
            arr.flatMap((x) => [x]), arr.toSpliced(1), [4, 5].toSorted(),
            arr.map((x) => x).toReversed(),
        );
        "#,
    );
    for intrinsic in [
        "ArrayToSorted",
        "ArrayToReversed",
        "ArrayReduceRight",
        "ArrayFlat {",
        "ArrayFlatMap",
        "ArrayToSpliced",
    ] {
        assert!(
            hir.contains(intrinsic),
            "a proven Array receiver lost the {intrinsic} intrinsic: {hir}"
        );
    }
}

#[test]
fn builtin_module_names_on_user_objects_stay_generic() {
    let hir = lower(
        r#"
        const crypto = { sha256(x: string) { return x; }, md5(x: string) { return x; } };
        const fs = { readFileSync(x: string) { return x; } };
        const path = { join(a: string, b: string) { return a + b; }, sep: "user-sep" };
        const os = { platform() { return "user-os"; }, EOL: "user-eol" };
        const net = { createServer() { return "user-server"; } };
        function viaParam(crypto: any) { return crypto.md5("param"); }
        console.log(
            crypto.sha256("x"), fs.readFileSync("x"), path.join("a", "b"),
            path.sep, os.platform(), os.EOL, net.createServer(), viaParam(crypto),
        );
        "#,
    );
    for intrinsic in [
        "CryptoSha256",
        "CryptoMd5",
        "FsReadFileBinary",
        "PathJoin",
        "PathSep",
        "OsPlatform",
        "OsEOL",
        "NetCreateServer",
    ] {
        assert!(
            !hir.contains(intrinsic),
            "a user object was lowered to the builtin intrinsic {intrinsic}: {hir}"
        );
    }
}

#[test]
fn imported_builtin_modules_keep_their_intrinsics() {
    let hir = lower(
        r#"
        import * as cryptoBuiltin from "node:crypto";
        import * as fsBuiltin from "node:fs";
        import * as pathBuiltin from "node:path";
        import * as osBuiltin from "node:os";
        import * as netBuiltin from "node:net";
        function shadowed(cryptoBuiltin: any) { return cryptoBuiltin.sha256("local"); }
        console.log(
            cryptoBuiltin.sha256("x"), fsBuiltin.readFileSync("x"), pathBuiltin.join("a", "b"),
            pathBuiltin.sep, osBuiltin.platform(), osBuiltin.EOL, netBuiltin.createServer(),
            shadowed({ sha256(value: string) { return value; } }),
        );
        "#,
    );
    for intrinsic in [
        "CryptoSha256",
        "FsReadFileBinary",
        "PathJoin",
        "PathSep",
        "OsPlatform",
        "OsEOL",
        "NetCreateServer",
    ] {
        assert!(
            hir.contains(intrinsic),
            "a builtin import lost the {intrinsic} intrinsic: {hir}"
        );
    }
    assert_eq!(
        hir.matches("CryptoSha256").count(),
        1,
        "an inner local must shadow the imported builtin alias: {hir}"
    );
}

#[test]
fn pipe_response_any_helper_uses_generic_method_dispatch() {
    let hir = lower(
        r#"
        function pipeResponse(response: any, dest: any): any {
            return response.pipe(dest);
        }
        "#,
    );
    assert!(
        hir.contains("PropertyGet") && hir.contains("property: \"pipe\""),
        "an any-typed response.pipe call should retain generic method lookup: {hir}"
    );
}

fn qualified_type_receiver_lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = perry_parser::parse_typescript_with_cache(
                &src,
                "qualified_type_receiver.ts",
                &mut cache,
            )
            .expect("qualified receiver syntax should parse");
            crate::lower_module(&parsed.module, "test", "qualified_type_receiver.ts")
                .expect("qualified receiver HIR should lower")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn qualified_type_receiver_return<'a>(module: &'a Module, name: &str) -> &'a crate::ir::Expr {
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("the source function must be present");
    function
        .body
        .iter()
        .find_map(|statement| match statement {
            crate::ir::Stmt::Return(Some(value)) => Some(value),
            _ => None,
        })
        .expect("the source function must return an expression")
}

fn qualified_type_receiver_assert_generic_join(value: &crate::ir::Expr, expected: &[&str]) {
    use crate::ir::Expr;
    let Expr::Call { callee, args, .. } = value else {
        panic!("an unproven receiver must retain a generic call, not an array fold: {value:?}");
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        panic!("join must retain receiver property lookup: {callee:?}");
    };
    assert_eq!(property, "join");
    assert!(matches!(object.as_ref(), Expr::LocalGet(_)));
    assert_eq!(
        args.len(),
        expected.len(),
        "all user arguments must survive HIR lowering"
    );
    for (argument, expected) in args.iter().zip(expected) {
        assert!(
            matches!(argument, Expr::String(actual) if actual.as_str() == *expected),
            "argument order and values must survive lowering: {argument:?}"
        );
    }
}

#[test]
fn qualified_type_receiver_named_join_keeps_both_arguments() {
    let module = qualified_type_receiver_lower(
        r#"function qualified(svc: Missing.Service) { return svc.join("a", "b"); }"#,
    );
    qualified_type_receiver_assert_generic_join(
        qualified_type_receiver_return(&module, "qualified"),
        &["a", "b"],
    );
}

#[test]
fn qualified_type_receiver_generic_join_keeps_both_arguments() {
    let module = qualified_type_receiver_lower(
        r#"function qualified(svc: Missing.Box<string>) { return svc.join("a", "b"); }"#,
    );
    qualified_type_receiver_assert_generic_join(
        qualified_type_receiver_return(&module, "qualified"),
        &["a", "b"],
    );
}

#[test]
fn qualified_type_receiver_array_tuple_and_resolved_alias_keep_array_join() {
    use crate::ir::Expr;
    let module = qualified_type_receiver_lower(
        r#"
        type Strings = string[];
        function array(svc: string[]) { return svc.join("-"); }
        function genericArray(svc: Array<string>) { return svc.join("-"); }
        function tuple(svc: [string, string]) { return svc.join("-"); }
        function alias(svc: Strings) { return svc.join("-"); }
    "#,
    );
    for name in ["array", "genericArray", "tuple", "alias"] {
        let value = qualified_type_receiver_return(&module, name);
        let Expr::ArrayJoin {
            array,
            separator: Some(separator),
        } = value
        else {
            panic!("a proven array must retain its existing join fold ({name}): {value:?}");
        };
        assert!(matches!(array.as_ref(), Expr::LocalGet(_)));
        assert!(matches!(separator.as_ref(), Expr::String(value) if value == "-"));
    }
}

#[test]
fn qualified_type_receiver_typed_arrays_keep_existing_join_routes() {
    use crate::ir::Expr;
    let module = qualified_type_receiver_lower(
        r#"
        function integers(svc: Int32Array) { return svc.join("-"); }
        function bytes(svc: Uint8Array) { return svc.join("-"); }
    "#,
    );
    let integer_join = qualified_type_receiver_return(&module, "integers");
    assert!(
        matches!(integer_join, Expr::ArrayJoin { .. }),
        "the existing Int32Array HIR route must remain unchanged: {integer_join:?}"
    );
    qualified_type_receiver_assert_generic_join(
        qualified_type_receiver_return(&module, "bytes"),
        &["-"],
    );
}

#[test]
fn qualified_type_receiver_known_objects_and_unknowns_do_not_fold_join() {
    use crate::ir::Expr;
    let module = qualified_type_receiver_lower(
        r#"
        class Service { join(a: string, b: string) { return a + b; } }
        interface Face { join(a: string, b: string): string; }
        function knownClass(svc: Service) { return svc.join("a", "b"); }
        function knownInterface(svc: Face) { return svc.join("a", "b"); }
        function dynamic(svc: any) { return svc.join("a", "b"); }
        function unknown(svc: unknown) { return svc.join("a", "b"); }
    "#,
    );
    for name in ["knownClass", "knownInterface", "dynamic", "unknown"] {
        let value = qualified_type_receiver_return(&module, name);
        assert!(
            !matches!(value, Expr::ArrayJoin { .. }),
            "an existing object/dynamic route must not become an array fold ({name}): {value:?}"
        );
    }
    for name in ["dynamic", "unknown"] {
        qualified_type_receiver_assert_generic_join(
            qualified_type_receiver_return(&module, name),
            &["a", "b"],
        );
    }
}

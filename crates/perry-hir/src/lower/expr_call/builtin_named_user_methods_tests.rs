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

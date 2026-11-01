//! ECMA-262 21.4: Date reads its internal time, coerces only setter
//! operands, and toJSON invokes this.toISOString(). Set compares keys.
use super::*;

fn check(body: &str, yes: bool) {
    let src = format!(r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.marker; }}
        const o: any = new Svc(); const d: any = new Date(0);
        {body}
        console.log(g(o));
    "#);
    let pf = facts(&src);
    let v = site(&pf, "marker").verdict.clone();
    assert_eq!(!matches!(v, Verdict::No(_)), yes, "{body}: {v:?}\n{}", pf.dump_text());
}

#[test]
fn date_setters_read_only() {
    for name in ["setTime", "setDate", "setFullYear", "setMonth", "setHours", "setMinutes", "setSeconds", "setMilliseconds", "setUTCDate", "setUTCFullYear", "setUTCMonth", "setUTCHours", "setUTCMinutes", "setUTCSeconds", "setUTCMilliseconds"] {
        check(&format!("d.{name}(o);"), true);
    }
}
#[test]
fn date_getters_ignore_arguments() {
    for name in ["getTime", "getDate", "getDay", "getFullYear", "getMonth", "getHours", "getMinutes", "getSeconds", "getMilliseconds", "getUTCDate", "getUTCDay", "getUTCFullYear", "getUTCMonth", "getUTCHours", "getUTCMinutes", "getUTCSeconds", "getUTCMilliseconds", "getTimezoneOffset", "valueOf", "toString", "toDateString", "toTimeString", "toUTCString", "toISOString"] {
        check(&format!("d.{name}(o);"), true);
    }
}
#[test]
fn date_typed_setter_and_statics_read_only() {
    check("const typed = new Date(0); typed.setTime(o); Date.parse(o); Date.UTC(o, o);", true);
}
#[test]
fn set_queries_do_not_touch_key() {
    for op in ["s.has(o)", "s.delete(o)", "s.size", "s.clear()"] { check(&format!("const s = new Set(); {op};"), true); }
}
#[test]
fn date_replaced_method_stays_conservative() {
    check("d.setTime = (x: any) => x.leak(); d.setTime(o);", false);
}
#[test]
fn date_unknown_receiver_stays_conservative() {
    check("const x: any = (globalThis as any).unknown; x.setTime(o);", false);
}
#[test]
fn date_setter_runs_coercion() {
    check("o.valueOf = function() { this.leak(); return 0; }; d.setTime(o);", false);
}
#[test]
fn date_json_runs_replaced_iso() {
    check("d.toISOString = () => { o.leak(); return ''; }; d.toJSON();", false);
}
#[test]
fn set_replaced_query_stays_conservative() {
    check("const s: any = new Set(); s.has = (x: any) => { x.leak(); return false; }; s.has(o);", false);
}

//! In-crate unit tests for the #6559 interpreter: subset semantics + the
//! host bridge in both directions. The heavier end-to-end proof (real ajv /
//! fast-json-stringify / find-my-way through a compiled binary) lives in
//! `crates/perry/tests/issue_6559_*.rs`.

use super::*;

fn dyn_fn(args: &[&str]) -> f64 {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    dyn_function_from_strings(&owned)
}

fn call(f: f64, args: &[f64]) -> f64 {
    unsafe { crate::closure::js_native_call_value(f, args.as_ptr(), args.len()) }
}

fn num(n: f64) -> f64 {
    f64::from_bits(crate::value::JSValue::number(n).bits())
}

fn string(s: &str) -> f64 {
    super::bridge::make_string(s)
}

fn as_str(v: f64) -> String {
    super::bridge::read_string(v).expect("expected a string value")
}

fn as_num(v: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        assert!(jv.is_number(), "expected a number, got bits {:x}", v.to_bits());
        v
    }
}

fn truthy(v: f64) -> bool {
    crate::value::js_is_truthy(v) != 0
}

/// Run `f` under a Rust-side landing pad; `Err(exception)` when it throws.
/// Same setjmp idiom as the interpreter's own try/catch.
fn catch_throw(f: impl FnOnce() -> f64) -> Result<f64, f64> {
    use crate::ffi::setjmp::setjmp;
    let trap = crate::exception::js_try_push();
    let jumped = unsafe { setjmp(trap as *mut std::os::raw::c_int) };
    if jumped == 0 {
        let v = f();
        crate::exception::js_try_end();
        Ok(v)
    } else {
        crate::exception::js_try_end();
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(exc)
    }
}

fn error_message(exc: f64) -> String {
    let msg = super::bridge::get_member(exc, "message");
    super::bridge::read_string(msg).unwrap_or_default()
}

// ── core evaluation ────────────────────────────────────────────────────────

#[test]
fn returns_arithmetic_on_parameters() {
    let f = dyn_fn(&["a", "b", "return a * 10 + b"]);
    let r = call(f, &[num(4.0), num(2.0)]);
    assert_eq!(as_num(r), 42.0);
}

#[test]
fn empty_body_returns_undefined_zod_probe() {
    // zod's JIT probe is `new Function("")` — it must now SUCCEED and yield a
    // callable that returns undefined.
    let f = dyn_fn(&[""]);
    let r = call(f, &[]);
    assert!(crate::value::JSValue::from_bits(r.to_bits()).is_undefined());
}

#[test]
fn loops_conditionals_and_compound_assignment() {
    let f = dyn_fn(&[
        "n",
        r#"
        let total = 0;
        for (let i = 0; i < n; i++) {
            if (i % 2 === 0) { total += i; } else { total -= 1; }
        }
        let j = n;
        while (j > 0) { j--; total += 1; }
        do { total += 100; } while (false);
        return total;
        "#,
    ]);
    // n=5: evens 0+2+4=6, odds -1*2=-2, +5 (while), +100 → 109
    let r = call(f, &[num(5.0)]);
    assert_eq!(as_num(r), 109.0);
}

#[test]
fn string_concat_and_template_literals() {
    let f = dyn_fn(&[
        "name",
        "n",
        r#"return `hello ${name}, you have ${n + 1} items` + "!";"#,
    ]);
    let r = call(f, &[string("ada"), num(2.0)]);
    assert_eq!(as_str(r), "hello ada, you have 3 items!");
}

#[test]
fn object_and_array_literals_with_member_access() {
    let f = dyn_fn(&[
        "k",
        r#"
        const obj = { a: 1, ["dyn" + "amic"]: 2, nested: { deep: [10, 20, 30] } };
        const arr = [obj.a, obj.dynamic, obj.nested.deep[k]];
        return arr[0] + arr[1] + arr[2];
        "#,
    ]);
    let r = call(f, &[num(2.0)]);
    assert_eq!(as_num(r), 33.0);
}

#[test]
fn shorthand_props_and_spread() {
    let f = dyn_fn(&[
        "iss",
        r#"
        const path = ["name"];
        const copy = { ...iss, path: ["prefix", ...path] };
        return copy.code + ":" + copy.path.join("/");
        "#,
    ]);
    let obj_idx = root_push(super::bridge::object_new());
    super::bridge::set_member(root_get(obj_idx), "code", string("invalid_type"));
    let r = call(f, &[root_get(obj_idx)]);
    roots_truncate(obj_idx);
    assert_eq!(as_str(r), "invalid_type:prefix/name");
}

#[test]
fn switch_with_fallthrough_and_default() {
    let f = dyn_fn(&[
        "x",
        r#"
        let out = "";
        switch (x) {
            case 1: out += "one ";
            case 2: out += "two"; break;
            case 3: out += "three"; break;
            default: out += "other";
        }
        return out;
        "#,
    ]);
    assert_eq!(as_str(call(f, &[num(1.0)])), "one two");
    let f2 = dyn_fn(&[
        "x",
        r#"
        switch (x) { case 3: return "three"; default: return "other"; }
        "#,
    ]);
    assert_eq!(as_str(call(f2, &[num(9.0)])), "other");
}

#[test]
fn labeled_break_out_of_nested_loops() {
    // ajv's uniqueItems duplicate scan: `outer0: for(;i--;){ for(j=i;j--;) {
    // … break outer0; } }`.
    let f = dyn_fn(&[
        "arr",
        r#"
        let i = arr.length;
        let j;
        let dup = -1;
        outer0:
        for (; i--;) {
            for (j = i; j--;) {
                if (arr[i] === arr[j]) { dup = i; break outer0; }
            }
        }
        return dup;
        "#,
    ]);
    let arr_idx = root_push(super::bridge::array_new());
    for v in ["a", "b", "a", "c"] {
        super::bridge::array_push_rooted(arr_idx, string(v));
    }
    let r = call(f, &[root_get(arr_idx)]);
    roots_truncate(arr_idx);
    assert_eq!(as_num(r), 2.0);
}

#[test]
fn for_in_enumerates_own_keys() {
    let f = dyn_fn(&[
        "data",
        r#"
        let out = "";
        for (const key in data) {
            if (!(key === "street" || key === "zip")) { out += key + ";"; }
        }
        return out;
        "#,
    ]);
    let obj_idx = root_push(super::bridge::object_new());
    super::bridge::set_member(root_get(obj_idx), "street", string("main"));
    super::bridge::set_member(root_get(obj_idx), "extra", num(1.0));
    super::bridge::set_member(root_get(obj_idx), "zip", string("12345"));
    let r = call(f, &[root_get(obj_idx)]);
    roots_truncate(obj_idx);
    assert_eq!(as_str(r), "extra;");
}

#[test]
fn typeof_equality_and_logical_ops() {
    let f = dyn_fn(&[
        "data",
        r#"
        return (typeof data == "number") && (!(data % 1) && !isNaN(data)) && isFinite(data);
        "#,
    ]);
    assert!(truthy(call(f, &[num(7.0)])));
    assert!(!truthy(call(f, &[num(7.5)])));
    assert!(!truthy(call(f, &[string("7")])));
}

#[test]
fn typeof_of_undeclared_identifier_does_not_throw() {
    let f = dyn_fn(&["return typeof totallyMissingIdent"]);
    assert_eq!(as_str(call(f, &[])), "undefined");
}

#[test]
fn ternary_nullish_and_optional_chaining() {
    let f = dyn_fn(&[
        "o",
        r#"return (o?.x ?? "fallback") + ":" + (o ? "y" : "n");"#,
    ]);
    let r = call(f, &[f64::from_bits(crate::value::TAG_NULL)]);
    assert_eq!(as_str(r), "fallback:n");
    let obj_idx = root_push(super::bridge::object_new());
    super::bridge::set_member(root_get(obj_idx), "x", string("val"));
    let r2 = call(f, &[root_get(obj_idx)]);
    roots_truncate(obj_idx);
    assert_eq!(as_str(r2), "val:y");
}

// ── functions, closures, recursion ─────────────────────────────────────────

#[test]
fn named_function_expression_recursion_and_expando() {
    // ajv's exact shape: `return function validateN(...) { …
    // validateN.errors = …; return … }` — the name binds inside the body and
    // expando properties stick on the returned closure.
    let f = dyn_fn(&[
        r#"
        return function fact(n) {
            if (n <= 1) { fact.calls = (fact.calls || 0) + 1; return 1; }
            return n * fact(n - 1);
        };
        "#,
    ]);
    let fact_idx = root_push(call(f, &[]));
    let r = call(root_get(fact_idx), &[num(5.0)]);
    assert_eq!(as_num(r), 120.0);
    let calls = super::bridge::get_member(root_get(fact_idx), "calls");
    roots_truncate(fact_idx);
    assert_eq!(as_num(calls), 1.0);
}

#[test]
fn closures_capture_interpreter_scope() {
    let f = dyn_fn(&[
        r#"
        let count = 0;
        const inc = function () { count += 1; return count; };
        inc(); inc();
        const get = () => count;
        return get();
        "#,
    ]);
    assert_eq!(as_num(call(f, &[])), 2.0);
}

#[test]
fn function_declarations_hoist() {
    let f = dyn_fn(&[
        "x",
        r#"
        return helper(x) + 1;
        function helper(v) { return v * 2; }
        "#,
    ]);
    assert_eq!(as_num(call(f, &[num(10.0)])), 21.0);
}

#[test]
fn default_and_destructured_parameters() {
    // The exact ajv validator signature shape:
    // `(data, {instancePath="", rootData=data} = {})`.
    let f = dyn_fn(&[
        "data",
        r#"{instancePath="", rootData=data}={}"#,
        r#"return instancePath + "|" + rootData;"#,
    ]);
    // Called with only `data` — the whole options object defaults.
    let r = call(f, &[string("D")]);
    assert_eq!(as_str(r), "|D");
    // Called with explicit options.
    let opts_idx = root_push(super::bridge::object_new());
    super::bridge::set_member(root_get(opts_idx), "instancePath", string("/name"));
    let r2 = call(f, &[string("D"), root_get(opts_idx)]);
    roots_truncate(opts_idx);
    assert_eq!(as_str(r2), "/name|D");
}

#[test]
fn var_hoisting_across_sibling_blocks() {
    // ajv re-`var`s `_valid0` in sibling blocks and reads it across them.
    let f = dyn_fn(&[
        "a",
        r#"
        if (a) { var _valid0 = "yes"; }
        else { var _valid0 = "no"; }
        return _valid0;
        "#,
    ]);
    assert_eq!(as_str(call(f, &[f64::from_bits(crate::value::TAG_TRUE)])), "yes");
}

#[test]
fn sloppy_assignment_to_undeclared_identifier() {
    // find-my-way's generated matcher assigns the never-declared `value`.
    let f = dyn_fn(&[
        "c",
        r#"
        value = c + 1;
        return value * 2;
        "#,
    ]);
    assert_eq!(as_num(call(f, &[num(20.0)])), 42.0);
}

// ── exceptions ─────────────────────────────────────────────────────────────

#[test]
fn try_catch_finally_inside_interpreted_code() {
    let f = dyn_fn(&[
        "mode",
        r#"
        let log = "";
        try {
            log += "t";
            if (mode === 1) { throw new TypeError("boom"); }
            log += "T";
        } catch (e) {
            log += "c:" + e.message;
        } finally {
            log += "f";
        }
        return log;
        "#,
    ]);
    assert_eq!(as_str(call(f, &[num(0.0)])), "tTf");
    assert_eq!(as_str(call(f, &[num(1.0)])), "tc:boomf");
}

#[test]
fn interpreted_throw_escapes_to_host_catch() {
    let f = dyn_fn(&["throw new Error(\"escaped\")"]);
    let result = catch_throw(|| call(f, &[]));
    let exc = result.expect_err("interpreted throw must reach the host trap");
    assert_eq!(error_message(exc), "escaped");
}

#[test]
fn parse_error_throws_syntax_error() {
    // The #5206 fixture shape: `new Function("return (not json)")` — invalid
    // source must throw a catchable error at CONSTRUCTION time, like Node.
    let result = catch_throw(|| dyn_fn(&["return (not json)"]));
    let exc = result.expect_err("invalid source must throw");
    let name = super::bridge::get_member(exc, "name");
    assert_eq!(as_str(name), "SyntaxError");
}

#[test]
fn unsupported_construct_diagnostic_names_the_construct() {
    let result = catch_throw(|| {
        let f = dyn_fn(&["return class {}"]);
        call(f, &[])
    });
    let exc = result.expect_err("class expression must be rejected");
    let msg = error_message(exc);
    assert!(
        msg.contains("unsupported construct") && msg.contains("class"),
        "diagnostic must name the construct; got: {msg}"
    );
}

#[test]
fn generator_body_rejected_at_construction() {
    let result = catch_throw(|| dyn_fn(&["return function* g() { yield 1; }"]));
    // Lazy or eager, it must throw with the construct named. (Nested fn
    // expressions are checked at evaluation.)
    let exc = match result {
        Err(e) => e,
        Ok(f) => catch_throw(|| call(f, &[])).expect_err("generator must be rejected"),
    };
    let msg = error_message(exc);
    assert!(msg.contains("generator"), "got: {msg}");
}

// ── host bridging: interpreted → host ──────────────────────────────────────

extern "C" fn host_double_thunk(
    _closure: *const crate::closure::ClosureHeader,
    v: f64,
) -> f64 {
    num(as_num_raw(v) * 2.0)
}

fn as_num_raw(v: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(v.to_bits());
    if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        v
    }
}

fn host_double_fn() -> f64 {
    let fp = host_double_thunk as *const u8;
    crate::closure::js_register_closure_arity(fp, 1);
    let closure = crate::closure::js_closure_alloc_singleton(fp);
    crate::value::js_nanbox_pointer(closure as i64)
}

#[test]
fn interpreted_code_calls_host_function_parameter() {
    // The ajv scope pattern: generated code calls real host functions handed
    // in through the constructor parameters (format validators, func2, …).
    let f = dyn_fn(&["dbl", "x", "return dbl(x) + dbl(2)"]);
    let r = call(f, &[host_double_fn(), num(20.0)]);
    assert_eq!(as_num(r), 44.0);
}

#[test]
fn interpreted_code_calls_host_object_methods() {
    // scope.schema[i] / vErrors.push(err) / key.replace(...) shapes: method
    // dispatch on host arrays and strings.
    let f = dyn_fn(&[
        "scope",
        r#"
        const arr = scope.list;
        arr.push("x");
        const joined = arr.join(",");
        return joined.replace("b", "B") + "|" + scope.list.length;
        "#,
    ]);
    let scope_idx = root_push(super::bridge::object_new());
    let list_idx = root_push(super::bridge::array_new());
    super::bridge::array_push_rooted(list_idx, string("a"));
    super::bridge::array_push_rooted(list_idx, string("b"));
    super::bridge::set_member(root_get(scope_idx), "list", root_get(list_idx));
    let r = call(f, &[root_get(scope_idx)]);
    roots_truncate(scope_idx);
    assert_eq!(as_str(r), "a,B,x|3");
}

#[test]
fn interpreted_code_constructs_host_class_parameter() {
    // find-my-way: `new Function('NullObject', 'const fn = function
    // _createParamsObject(paramsArray){ const params = new NullObject(); … }')`
    // — `new` on a host value received as a parameter. Host stand-in: a
    // closure used as a constructor via the runtime's construct path is out
    // of unit-test scope (needs codegen class metadata), so this exercises
    // the interpreted-constructor path instead: `new` on an *interpreted*
    // function parameter.
    let make_ctor = dyn_fn(&[r#"
        return function Pair(a, b) { this.a = a; this.b = b; };
    "#]);
    let ctor_idx = root_push(call(make_ctor, &[]));
    let f = dyn_fn(&[
        "Ctor",
        r#"
        const p = new Ctor(1, 2);
        return p.a + p.b;
        "#,
    ]);
    let r = call(f, &[root_get(ctor_idx)]);
    roots_truncate(ctor_idx);
    assert_eq!(as_num(r), 3.0);
}

#[test]
fn regex_literal_and_string_replace() {
    // ajv: `key.replace(/~/g, "~0").replace(/\//g, "~1")`.
    let f = dyn_fn(&[
        "key",
        r#"return key.replace(/~/g, "~0").replace(/\//g, "~1");"#,
    ]);
    let r = call(f, &[string("a~b/c~d")]);
    assert_eq!(as_str(r), "a~0b~1c~0d");
}

#[test]
fn charcodeat_prefix_matcher_find_my_way_shape() {
    // Verbatim find-my-way matchPrefix codegen.
    let f = dyn_fn(&[
        "path",
        "i",
        "return path.charCodeAt(i + 1) === 115 && path.charCodeAt(i + 2) === 101",
    ]);
    assert!(truthy(call(f, &[string("/se/x"), num(0.0)])));
    assert!(!truthy(call(f, &[string("/xx/x"), num(0.0)])));
}

// ── host bridging: host → interpreted ──────────────────────────────────────

#[test]
fn host_calls_interpreted_function_stored_as_property() {
    // ajv root0.validate pattern: an interpreted closure stored as a host
    // object property, invoked as a method → `this` = receiver.
    let make = dyn_fn(&[r#"
        return function () { return this.tag + "!"; };
    "#]);
    let m_idx = root_push(call(make, &[]));
    let obj_idx = root_push(super::bridge::object_new());
    super::bridge::set_member(root_get(obj_idx), "tag", string("recv"));
    super::bridge::set_member(root_get(obj_idx), "speak", root_get(m_idx));
    // Host-style method dispatch (what compiled `obj.speak()` lowers to).
    let r = super::bridge::call_method(root_get(obj_idx), "speak", &[]);
    roots_truncate(m_idx);
    assert_eq!(as_str(r), "recv!");
}

#[test]
fn interpreted_this_binding_via_call() {
    // find-my-way getMatchingHandler shape: `return this.handlers[...]`,
    // invoked with an explicit receiver.
    let f = dyn_fn(&["i", "return this.handlers[i]"]);
    let obj_idx = root_push(super::bridge::object_new());
    let handlers_idx = root_push(super::bridge::array_new());
    super::bridge::array_push_rooted(handlers_idx, string("h0"));
    super::bridge::array_push_rooted(handlers_idx, string("h1"));
    super::bridge::set_member(root_get(obj_idx), "handlers", root_get(handlers_idx));
    super::bridge::set_member(root_get(obj_idx), "pick", f);
    let r = super::bridge::call_method(root_get(obj_idx), "pick", &[num(1.0)]);
    roots_truncate(obj_idx);
    assert_eq!(as_str(r), "h1");
}

#[test]
fn math_and_bitwise_find_my_way_constrainer_shape() {
    // Verbatim deriveSyncConstraints/getMatchingHandler arithmetic:
    // `31 - Math.clz32(candidates)`, `candidates &= mask`.
    let f = dyn_fn(&[
        "n",
        r#"
        let candidates = n;
        let mask = -2;
        candidates &= mask;
        return 31 - Math.clz32(candidates);
        "#,
    ]);
    let r = call(f, &[num(5.0)]);
    // 5 & -2 = 4; clz32(4) = 29; 31-29 = 2.
    assert_eq!(as_num(r), 2.0);
}

#[test]
fn multiple_param_decls_in_one_string() {
    // `new Function("a, b", "c", body)` — V8 joins parameter strings.
    let f = dyn_fn(&["a, b", "c", "return a + b + c"]);
    assert_eq!(as_num(call(f, &[num(1.0), num(2.0), num(3.0)])), 6.0);
}

#[test]
fn depd_fast_path_still_intact() {
    // The depd wrapper shape must keep its non-interpreted fast path (it
    // uses `arguments`, which the interpreter deliberately rejects) — guard
    // that the interpreter path doesn't regress it by checking `arguments`
    // stays a named diagnostic.
    let f = dyn_fn(&["fn", "return function () { return fn.apply(this, arguments); }"]);
    let wrapper_maker_result = catch_throw(|| {
        let wrapped_idx = root_push(call(f, &[host_double_fn()]));
        let r = call(root_get(wrapped_idx), &[num(21.0)]);
        roots_truncate(wrapped_idx);
        r
    });
    match wrapper_maker_result {
        Ok(v) => assert_eq!(as_num(v), 42.0),
        Err(exc) => {
            let msg = error_message(exc);
            assert!(msg.contains("arguments"), "got: {msg}");
        }
    }
}

#[test]
fn probe_concat_minimal() {
    let f = dyn_fn(&[r#"return "a" + "b";"#]);
    let r = call(f, &[]);
    assert_eq!(as_str(r), "ab");
}

#[test]
fn probe_concat_var() {
    let f = dyn_fn(&[r#"let s = ""; s += "x"; s = s + "y"; return s;"#]);
    let r = call(f, &[]);
    assert_eq!(as_str(r), "xy");
}

#[test]
fn finally_runs_when_catch_throws() {
    let f = dyn_fn(&[
        "log",
        r#"
        try {
            try { throw new Error("first"); }
            catch (e) { log.push("catch"); throw new Error("second"); }
            finally { log.push("finally"); }
        } catch (e2) {
            log.push("outer:" + e2.message);
        }
        return log.join(",");
        "#,
    ]);
    let log_idx = root_push(super::bridge::array_new());
    let r = call(f, &[root_get(log_idx)]);
    roots_truncate(log_idx);
    assert_eq!(as_str(r), "catch,finally,outer:second");
}

#[test]
fn try_finally_without_catch_reruns_finalizer_then_rethrows() {
    let f = dyn_fn(&[
        "log",
        r#"
        try {
            try { throw new Error("boom"); }
            finally { log.push("cleanup"); }
        } catch (e) {
            log.push("caught:" + e.message);
        }
        return log.join(",");
        "#,
    ]);
    let log_idx = root_push(super::bridge::array_new());
    let r = call(f, &[root_get(log_idx)]);
    roots_truncate(log_idx);
    assert_eq!(as_str(r), "cleanup,caught:boom");
}

#[test]
fn probe_compiled_path_reads_closure_expando_null() {
    // Mirror the COMPILED-code read path (`js_object_get_field_by_name_f64`,
    // what codegen emits for `validate.errors`) against a null stored by the
    // interpreter (`validate10.errors = vErrors` with vErrors === null).
    let f = dyn_fn(&[r#"
        const g = function v() { v.errors = null; return true; };
        g();
        return g;
    "#]);
    let g_idx = root_push(call(f, &[]));
    let key = crate::string::js_string_from_bytes("errors".as_ptr(), 6);
    let obj = crate::value::js_nanbox_get_pointer(root_get(g_idx))
        as *const crate::object::ObjectHeader;
    let read = crate::object::js_object_get_field_by_name_f64(obj, key);
    roots_truncate(g_idx);
    let jv = crate::value::JSValue::from_bits(read.to_bits());
    assert!(
        jv.is_null(),
        "compiled-path read of stored null must be null; got bits {:x}",
        read.to_bits()
    );
}

#[test]
fn probe_all_readers_of_null_closure_expando() {
    let f = dyn_fn(&[r#"
        const g = function v() { v.errors = null; return true; };
        g();
        return g;
    "#]);
    let g_idx = root_push(call(f, &[]));
    let g = root_get(g_idx);
    let ptr = crate::value::js_nanbox_get_pointer(g) as *const crate::object::ObjectHeader;
    let key = crate::string::js_string_from_bytes("errors".as_ptr(), 6);

    let deep = crate::object::js_object_get_field_by_name(ptr, key);
    eprintln!("deep getter bits:  {:016x}", deep.bits());

    let key2 = crate::string::js_string_from_bytes("errors".as_ptr(), 6);
    let f64r = crate::object::js_object_get_field_by_name_f64(ptr, key2);
    eprintln!("ic_miss bits:      {:016x}", f64r.to_bits());

    let key_v = super::bridge::make_string("errors");
    let dyn_r = crate::value::js_dyn_index_get(root_get(g_idx), key_v);
    eprintln!("dyn_index bits:    {:016x}", dyn_r.to_bits());

    let prop_r = unsafe {
        crate::value::js_get_property(root_get(g_idx), "errors".as_ptr() as i64, 6)
    };
    eprintln!("get_property bits: {:016x}", prop_r.to_bits());
    roots_truncate(g_idx);

    for (name, bits) in [
        ("deep", deep.bits()),
        ("ic_miss", f64r.to_bits()),
        ("dyn_index", dyn_r.to_bits()),
        ("get_property", prop_r.to_bits()),
    ] {
        assert_eq!(
            bits,
            crate::value::TAG_NULL,
            "{name} reader must yield canonical null"
        );
    }
}

use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn run_module(module: Module) -> String {
    run_module_after(module, "")
}

fn run_module_after(module: Module, after: &str) -> String {
    run_modules_after(&[("scoped".into(), module)], after)
}

fn run_modules_after(modules: &[(String, Module)], after: &str) -> String {
    let output = emit::compile_to_wasm_with_async(modules);
    let b64 = BASE64.encode(output.wasm_bytes);
    let script = format!(
        r#"
globalThis.window = globalThis;
const stub = {{appendChild() {{}}, style: {{}}}};
globalThis.document = {{createElement: () => ({{...stub}}), getElementById: () => null, head: stub, body: stub}};
(async () => {{
{runtime}
const __asyncFuncImpls = {{{async_js}}};
await bootPerryWasm("{b64}");
{after}
await new Promise(resolve => setImmediate(resolve));
}})().catch(e => {{ console.error(e); process.exitCode = 1; }});
"#,
        runtime = WASM_RUNTIME_JS,
        async_js = output.async_js
    );
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node is required for WASM semantics tests");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap().trim().to_string()
}

fn run_ts(source: &str) -> String {
    run_ts_after(source, "")
}

fn run_ts_after(source: &str, after: &str) -> String {
    let ast = perry_parser::parse_typescript(source, "scoped.ts").unwrap();
    let module = perry_hir::lower::lower_module(&ast, "scoped", "scoped.ts").unwrap();
    run_module_after(module, after)
}

#[test]
fn optional_bases_fields_strings_closures_and_recursive_calls_execute_once() {
    let output = run_ts(
        r#"
let calls = 0;
const cached = {value: 7};
function take() { calls++; return cached; }
console.log(take()?.value, calls);
function recurse(n: number): number {
  return n === 0 ? 0 : take()?.value + recurse(n - 1);
}
console.log(recurse(3), calls);
class WithField { value = take()?.value; constructor() {} }
console.log(new WithField().value, calls);
const text = {name: 'present'};
function getText() { return text; }
console.log(getText()?.name);
console.log((() => ({value: 5}))()?.value);
const nil: any = null;
function empty() { calls++; return nil; }
console.log(empty()?.value, calls);
"#,
    );
    assert_eq!(output, "7\n1\n21\n4\n7\n5\npresent\n5\nundefined\n6");
}

#[test]
fn optional_call_receiver_executes_once_and_preserves_this() {
    let output = run_ts(
        r#"
let calls = 0;
class Receiver { x = 3; constructor() {} read(n: number): number { return this.x + n; } }
const receiver = new Receiver();
function take(): Receiver { calls++; return receiver; }
console.log(take()?.read(4), calls);
console.log(take()?.read?.(5), calls);
"#,
    );
    assert_eq!(output, "7\n1\n8\n2");
}

#[test]
fn scoped_binding_shadows_module_global_only_inside_body_and_restores_outer() {
    use perry_hir::ir::*;
    use perry_hir::types::Type;
    let mut module = Module::new("scoped");
    module.init.push(Stmt::Let {
        id: 5,
        name: "outer".into(),
        ty: Type::Number,
        mutable: true,
        init: Some(Expr::Number(9.0)),
    });
    let log = |value| {
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(Expr::GlobalGet(999)),
                property: "log".into(),
            }),
            args: vec![value],
            type_args: vec![],
            byte_offset: 0,
        })
    };
    let inner = Expr::ScopedTemp {
        id: 5,
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(Expr::Number(1.0)),
        }),
        body: Box::new(Expr::LocalGet(5)),
    };
    module.init.push(log(Expr::ScopedTemp {
        id: 5,
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::LocalGet(5)),
            right: Box::new(Expr::Number(1.0)),
        }),
        body: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(inner),
            right: Box::new(Expr::LocalGet(5)),
        }),
    }));
    module.init.push(log(Expr::LocalGet(5)));
    assert_eq!(run_module(module), "21\n9");
}

#[test]
fn async_js_fallback_uses_activation_locals_for_awaited_capture() {
    let output = run_ts_after(
        r#"
let calls = 0;
const cached = {value: 7};
function take() { calls++; return cached; }
function g() { return take(); }
async function read() { return (await g())?.value; }
const pending = read();
console.log(calls);
"#,
        // Observe the existing Promise bridge directly: WASM's preexisting
        // async fallback does not implement Promise.then callbacks.
        r#"
const names = Object.keys(wasmInstance.exports).filter(name => name.startsWith('__wasm_global_'));
names.sort((a, b) => Number(a.slice(14)) - Number(b.slice(14)));
const pending = toJsValue(u64ToF64(wasmInstance.exports[names.at(-1)].value));
console.log(await pending);
"#,
    );
    assert_eq!(output, "1\n7");
}

#[test]
fn optional_shift_call_consumes_only_one_queue_entry() {
    let output = run_ts(
        r#"
const queue = [() => 11, () => 22];
console.log(queue.shift()?.());
console.log(queue.length);
"#,
    );
    assert_eq!(output, "11\n1");
}

#[test]
fn scoped_capture_in_loop_condition_and_static_initializer() {
    let output = run_ts(
        r#"
let calls = 0;
const cached = {value: 7};
function take() { calls++; return calls < 4 ? cached : null; }
let iterations = 0;
while (take()?.value) { iterations++; }
console.log(iterations, calls);
class Static { static value = (() => ({text: 'static'}))()?.text; }
console.log(Static.value);
"#,
    );
    assert_eq!(output, "3\n4\nstatic");
}

#[test]
fn async_scoped_bases_survive_suspension_without_cross_activation_state() {
    let output = run_ts_after(
        r#"
let calls = 0;
function g(n: number) { calls++; return [n]; }
function index() { return 0; }
async function read(n: number) { return (await g(n))?.[await index()]; }
const first = read(7);
const second = read(9);
console.log(calls);
"#,
        r#"
const names = Object.keys(wasmInstance.exports).filter(name => name.startsWith('__wasm_global_'));
names.sort((a, b) => Number(a.slice(14)) - Number(b.slice(14)));
const pending = names.slice(-2).map(name => toJsValue(u64ToF64(wasmInstance.exports[name].value)));
console.log((await Promise.all(pending)).join(','));
"#,
    );
    assert_eq!(output, "2\n7,9");
}

#[test]
fn host_interop_extracted_functions_and_perry_closures() {
    assert_eq!(
        run_ts(
            r#"
const parse = globalThis.parseInt;
console.log(parse('42'));
const identity = (v: number) => v + 1;
console.log(identity(4));
"#
        ),
        "42\n5"
    );
}

#[test]
fn host_interop_callback_arity_and_array_fallback() {
    assert_eq!(
        run_ts(
            r#"
async function make(): Promise<number> { return 41; }
make().then((v, omitted) => { console.log(v + 1); console.log(omitted); });
const xs: any = [1, 2, 3];
console.log(xs.findLast((v: number) => v < 3));
"#
        ),
        "2\n42\nundefined"
    );
}

#[test]
fn host_interop_receiver_identity_getters_and_ui_dispatch() {
    assert_eq!(
        run_ts_after(
            "",
            r#"
const assert = require('node:assert/strict');
const originalGlobal = globalThis;
const globalBits = __jsValueToBits(originalGlobal);
for (let i = 0; i < 100; i++) assert.equal(__jsValueToBits(originalGlobal), globalBits);
assert.equal(__classDispatch(new Headers({'x-test': 'value'}), 'get', ['x-test']), 'value');
let reads = 0;
const obj = {value: 7, get read() { reads++; return reads === 1 ? function() { return this.value; } : undefined; }};
assert.equal(__classDispatch(obj, 'read', []), 7);
assert.equal(reads, 1);
const state = uiStateCreate(9);
assert.equal(__classDispatch(state, 'get', []), 9);
const native = v => v + 1;
assert.equal(__classDispatch([1, 2], 'findLast', [native]), 2);
console.log('ok');
"#
        ),
        "ok"
    );
}

#[test]
fn host_interop_callback_identity_for_listener_removal() {
    assert_eq!(
        run_ts_after(
            r#"
const callback = () => { console.log('unexpected callback'); };
globalThis.savedCallback = callback;
"#,
            r#"
const assert = require('node:assert/strict');
const target = new EventTarget();
__classDispatch(target, 'addEventListener', ['test', globalThis.savedCallback]);
__classDispatch(target, 'removeEventListener', ['test', globalThis.savedCallback]);
assert.equal(target.dispatchEvent(new Event('test')), true);
console.log('removed');
"#
        ),
        "removed"
    );
}

#[test]
fn host_interop_bare_call_ignores_replaced_global_this_property() {
    assert_eq!(
        run_ts(
            r#"
globalThis.globalThis = {fetch: () => { console.log('wrong receiver'); }};
fetch('data:text/plain,hello').then((r) => r.text()).then((text) => { console.log(text); });
"#
        ),
        "hello"
    );
}

#[test]
fn host_interop_native_exceptions_reach_perry_catch() {
    assert_eq!(
        run_ts(
            r#"
const decode = globalThis.decodeURIComponent;
try { decode('%'); }
catch (error) { console.log('caught'); }
console.log(decode('ok'));
"#
        ),
        "caught\nok"
    );
    assert_eq!(
        run_ts_after(
            "",
            r#"
const assert = require('node:assert/strict');
for (const name of ['closure_call_0', 'closure_call_1', 'closure_call_2', 'closure_call_3', 'closure_call_spread']) {
  const error = new Error(name);
  const fail = () => { throw error; };
  const args = name.endsWith('spread') ? [[1, 2, 3, 4]] : [1, 2, 3];
  __memDispatch.try_start();
  assert.equal(__memDispatch[name](fail, ...args), undefined);
  assert.equal(__memDispatch.has_exception(), 1);
  assert.equal(__memDispatch.get_exception(), error);
  __memDispatch.try_end();
  assert.throws(() => __memDispatch[name](fail, ...args), thrown => thrown === error);
}
console.log('ok');
"#
        ),
        "ok"
    );
}

#[test]
fn host_interop_worker_reuses_reference_handles() {
    let bootstrap = WASM_RUNTIME_JS
        .split("const WORKER_BOOTSTRAP = `")
        .nth(1)
        .unwrap()
        .split("`;")
        .next()
        .unwrap();
    let source = BASE64.encode(bootstrap);
    assert_eq!(
        run_ts_after(
            "",
            &format!(
                r#"
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = Buffer.from('{source}', 'base64').toString();
vm.runInNewContext(source + `
const object = {{value: 7}};
const array = [object];
const fn = () => {{}};
for (const value of [object, array, fn]) {{
  const id = allocHandle(value);
  for (let i = 0; i < 100; i++) assert.equal(allocHandle(value), id);
  assert.equal(handleStore.get(id), value);
}}
assert.notEqual(allocHandle({{}}), allocHandle({{}}));
assert.equal(__bitsToJsValue(__jsValueToBits(object)), object);
`, {{self: {{}}, assert}});
console.log('ok');
"#
            )
        ),
        "ok"
    );
}

#[test]
fn host_interop_host_methods_enter_perry_catch() {
    assert_eq!(
        run_ts(
            r#"
const json: any = globalThis.JSON;
try { json.parse('{'); }
catch (error) { console.log('method caught'); }
console.log(json.parse('7'));
"#
        ),
        "method caught\n7"
    );
}

#[test]
fn host_interop_returned_calls_enter_catch_and_release_try_state() {
    assert_eq!(
        run_ts_after(
            r#"
function read() {
  const decode = globalThis.decodeURIComponent;
  try { return decode('%'); }
  catch (error) { return 'caught'; }
}
console.log(read());
function success() {
  const decode = globalThis.decodeURIComponent;
  try { return decode('ok'); }
  catch (error) { return 'unexpected'; }
}
console.log(success());
"#,
            r#"
const assert = require('node:assert/strict');
assert.equal(tryDepth, 0);
assert.equal(__memDispatch.has_exception(), 0);
"#
        ),
        "caught\nok"
    );
}

#[test]
fn host_interop_null_exception_is_not_lost() {
    assert_eq!(
        run_ts(
            r#"
const create = globalThis.Function;
const fail = create('throw null');
try { fail(); }
catch (error) { console.log(error === null); }
"#
        ),
        "true"
    );
}

#[test]
fn host_interop_catch_precedes_expression_effects_and_preserves_nesting() {
    assert_eq!(run_ts_after(r#"
const decode = globalThis.decodeURIComponent;
let calls = 0;
function later() { calls++; return 'later'; }
function join(a: any, b: any) { return a + b; }
try { join('prefix' + decode('%'), later()); }
catch (error) { console.log('arguments caught'); }
console.log(calls);
try { const value = true ? decode('%') : later(); console.log(value); }
catch (error) { console.log('conditional caught'); }
try { const value = true && decode('%'); console.log(value); }
catch (error) { console.log('logical caught'); }
try {
  try { decode('%'); }
  catch (error) { console.log('inner caught'); }
  console.log('outer continued');
} catch (error) { console.log('unexpected outer catch'); }
finally { console.log('finally'); }
console.log(decode('ok'));
"#, r#"
const assert = require('node:assert/strict');
assert.equal(tryDepth, 0);
assert.equal(__memDispatch.has_exception(), 0);
"#), "arguments caught\n0\nconditional caught\nlogical caught\ninner caught\nouter continued\nfinally\nok");
}

#[test]
fn host_interop_caught_nullish_values_and_method_receivers_are_preserved() {
    assert_eq!(
        run_ts(
            r#"
const create = globalThis.Function;
const fail = create('throw undefined');
try { fail(); }
catch (error) { console.log(error === undefined); }
const make = create('return { value: 7, read() { if (this.value !== 7) throw null; throw this.value; } }');
const object = make();
try { object.read(); }
catch (error) { console.log(error); }
const getter = create('return { get read() { throw null; } }')();
try { getter.read(); }
catch (error) { console.log(error === null); }
"#
        ),
        "true\n7\ntrue"
    );
}

#[test]
fn host_interop_nested_calls_and_loops_preserve_exception_frames() {
    assert_eq!(
        run_ts_after(
            r#"
const decode = globalThis.decodeURIComponent;
let effects = 0;
function later() { effects++; return 10; }
const maker = globalThis.Function;
const host = maker('return {sum(a,b,c){return a+b+c}}')();
for (let i = 0; i < 3; i++) {
  try { host.sum(1, decode('%'), later()); }
  catch (error) { console.log(i); }
}
console.log(effects);
console.log(host.sum(1,2,3));
try { while (true) { if (true) { decode('%'); } console.log('wrong'); } }
catch (error) { console.log('loop caught'); }
function nestedReturn() {
 try { try { return decode('good'); } catch (error) { return 'bad'; } }
 catch (error) { return 'outer bad'; }
}
console.log(nestedReturn());
try { try { decode('%'); } catch (error) { decode('%'); } }
catch (error) { console.log('outer caught'); }
console.log(host.sum(4,5,6));
"#,
            r#"
const assert = require('node:assert/strict');
assert.equal(tryDepth,0);
assert.equal(__memDispatch.has_exception(),0);
"#
        ),
        "0\n1\n2\n0\n6\nloop caught\ngood\nouter caught\n15"
    );
}

#[test]
fn host_interop_global_getters_and_unhandled_callees_stop_at_catch() {
    assert_eq!(
        run_ts_after(
            r#"
const create = globalThis.Function;
const install = create('Object.defineProperty(globalThis, "failingGlobal", {get() { throw 9; }, configurable: true})');
install();
const configure = create('const d = Object.getOwnPropertyDescriptor(globalThis, "Promise"); Object.defineProperty(globalThis, "Promise", {get() { throw 10; }, configurable: true}); return () => Object.defineProperty(globalThis, "Promise", d)');
const restore = configure();
try { const P = Promise; console.log(P); }
catch (error) { console.log(error); }
restore();
try { console.log(globalThis.failingGlobal); console.log('wrong getter'); }
catch (error) { console.log(error); }
const decode = globalThis.decodeURIComponent;
function helper() { decode('%'); console.log('wrong helper'); }
try { helper(); console.log('wrong caller'); }
catch (error) { console.log('helper caught'); }
"#,
            "require('node:assert/strict').equal(tryDepth, 0);"
        ),
        "10\n9\nhelper caught"
    );
}

#[test]
fn host_interop_explicit_throw_and_loop_exits_release_try_scopes() {
    assert_eq!(
        run_ts_after(
            r#"
function throws() {
  try { throw 7; }
  catch (error) { return error; }
}
console.log(throws());
for (let i = 0; i < 3; i++) {
  try { if (i === 0) continue; break; }
  finally { console.log('cleanup'); }
}
try { for (let j = 0; j < 2; j++) { if (j === 0) continue; break; } throw 8; }
catch (error) { console.log(error); }
"#,
            "require('node:assert/strict').equal(tryDepth, 0);"
        ),
        "7\ncleanup\ncleanup\n8"
    );
}

#[test]
fn host_interop_finally_preserves_and_replaces_pending_completion() {
    assert_eq!(
        run_ts_after(
            r#"
const decode = globalThis.decodeURIComponent;
try {
  try { decode('%'); }
  finally { console.log('cleanup one'); console.log('cleanup two'); }
} catch (error) { console.log('outer caught'); }
try {
  try { throw 3; }
  finally { throw 4; }
} catch (error) { console.log(error); }
function returned() {
  try { return 'value'; }
  finally { console.log('return cleanup'); }
}
console.log(returned());
function override() { try { return 'old'; } finally { return 'new'; } }
console.log(override());
"#,
            "require('node:assert/strict').equal(tryDepth, 0);"
        ),
        "cleanup one\ncleanup two\nouter caught\n4\nreturn cleanup\nvalue\nnew"
    );
}

#[test]
fn host_interop_imported_callees_stop_before_following_effects() {
    for import in [
        "import { fail } from './helper';",
        "import * as H from './helper';",
    ] {
        let call = if import.contains("* as") {
            "H.fail()"
        } else {
            "fail()"
        };
        let sources = [
            ("helper", "export function fail() { throw 7; console.log('wrong callee'); }".to_string()),
            ("entry", format!("{import} try {{ {call}; console.log('wrong caller'); }} catch (error) {{ console.log(error); }}")),
        ];
        let modules: Vec<_> = sources
            .iter()
            .map(|(name, source)| {
                let filename = format!("{name}.ts");
                let ast = perry_parser::parse_typescript(source, &filename).unwrap();
                let module = perry_hir::lower::lower_module(&ast, name, &filename).unwrap();
                (name.to_string(), module)
            })
            .collect();
        assert_eq!(
            run_modules_after(
                &modules,
                "require('node:assert/strict').equal(tryDepth, 0);"
            ),
            "7"
        );
    }
}

#[test]
fn host_interop_abrupt_scopes_and_function_signatures() {
    assert_eq!(run_ts_after(r#"
const decode = globalThis.decodeURIComponent;
function noValue() { decode('%'); console.log('wrong void'); }
function value() { decode('%'); return 'wrong value'; }
try { noValue(); } catch (e) { console.log('void caught'); }
try { console.log(value()); } catch (e) { console.log('value caught'); }
class Fail { constructor() { decode('%'); console.log('wrong constructor'); } static run() { decode('%'); console.log('wrong static'); } }
try { const f = new Fail(); } catch (e) { console.log('constructor caught'); }
try { Fail.run(); } catch (e) { console.log('static caught'); }
function nested() { try { try { return 1; } finally { console.log('inner final'); } } finally { console.log('outer final'); } }
console.log(nested());
try { try { throw 2; } catch (e) { throw 3; } finally { console.log('catch final'); } } catch (e) { console.log(e); }
function overridden() { try { throw 4; } finally { return 5; } }
console.log(overridden());
outer: for (let i = 0; i < 2; i++) {
 try { for (let j = 0; j < 2; j++) { if (j === 0) continue; break; } if (i === 0) continue outer; break outer; }
 finally { console.log('label cleanup'); }
}
for(let i=0;i<2;i++) { try { throw 8; } finally { break; } }
console.log('break suppressed');
try { for(let i=0;i<2;i++) { if(i===0) continue; break; } throw 9; } catch(e) { console.log(e); }
console.log(decode('done'));
"#, r#"const a=require('node:assert/strict');a.equal(tryDepth,0);a.equal(__memDispatch.has_exception(),0);"#),"void caught\nvalue caught\nconstructor caught\nstatic caught\ninner final\nouter final\n1\ncatch final\n3\n5\nlabel cleanup\nlabel cleanup\nbreak suppressed\n9\ndone");
}

#[test]
fn host_interop_throwing_callback_stops_the_host_iteration() {
    assert_eq!(
        run_ts(
            r#"
const xs: any = [1, 2, 3];
let calls = 0;
try { xs.findLast((value) => { calls++; throw 7; }); }
catch (error) { console.log(error); }
console.log(calls);
"#
        ),
        "7\n1"
    );
}

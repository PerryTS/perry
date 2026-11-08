use super::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn run_module(module: Module) -> String {
    run_module_after(module, "")
}

fn run_module_after(module: Module, after: &str) -> String {
    let output = emit::compile_to_wasm_with_async(&[("scoped".into(), module)]);
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

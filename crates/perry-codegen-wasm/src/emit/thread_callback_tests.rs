//! `perry/thread` callbacks on the web target.

use super::{run_web_source, run_web_source_with_dom, STUB_DOM};

// A thread callback that declares more parameters than it is passed receives undefined
// in the rest, as every other table call does (#15140).
#[test]
fn a_thread_callback_with_unpassed_parameters_receives_undefined() {
    assert_eq!(
        run_web_source(
            r#"
import { parallelFilter, parallelMap, spawn } from "perry/thread";
const k = 1;
console.log(parallelMap([1, 2], (n: number, m?: number) => n + k + ":" + m).join(","));
console.log(parallelFilter([1, 2], (n: number, m?: number) => m === undefined && n > k).join(","));
spawn((m?: number) => "spawn " + m).then((v: string) => console.log(v));
"#
        ),
        "2:undefined,3:undefined\n2\nspawn undefined"
    );
}

#[test]
fn a_rest_parameter_thread_callback_receives_its_arguments_as_an_array() {
    assert_eq!(
        run_web_source(
            r#"
import { parallelFilter, parallelMap, spawn } from "perry/thread";
const k = 1;
console.log(parallelMap([1, 2], (...r: number[]) => r.length + ":" + r[0]).join(","));
console.log(parallelFilter([1, 2], (...r: number[]) => r.length === k && r[0] > k).join(","));
spawn((...r: number[]) => "spawn " + r.length).then((v: string) => console.log(v));
"#
        ),
        "1:1,1:2\n2\nspawn 0"
    );
}

/// A `Worker` that runs the bootstrap in a fresh realm in-process, and the module's
/// base64 the HTML wrapper would leave on `window`, so `perry/thread` takes the worker path.
const IN_PROCESS_WORKER: &str = r#"
const vm = require('node:vm');
const { resolveObjectURL } = require('node:buffer');
const decode = atob;
globalThis.atob = s => { globalThis.__perryWasmB64 ??= s; return decode(s); };
globalThis.Worker = class {
  constructor(url) {
    this.listeners = [];
    const self = { postMessage: data => setImmediate(() => [...this.listeners].forEach(l => l({ data: structuredClone(data) }))) };
    this.scope = resolveObjectURL(url).text().then(src => { vm.runInNewContext(src, { self, TextDecoder }); return self; });
  }
  addEventListener(_, fn) { this.listeners.push(fn); }
  removeEventListener(_, fn) { this.listeners = this.listeners.filter(l => l !== fn); }
  postMessage(data) { this.scope.then(self => self.onmessage({ data: structuredClone(data) })); }
};
"#;

#[test]
fn a_rest_parameter_worker_callback_receives_its_arguments_as_an_array() {
    let source = format!("{STUB_DOM}{IN_PROCESS_WORKER}");
    assert_eq!(
        run_web_source_with_dom(
            r#"
import { parallelMap } from "perry/thread";
const xs: number[] = [];
for (let i = 0; i < 1024; i++) xs.push(i);
Promise.resolve(parallelMap(xs, (...r: number[]) => r)).then((out: any) => console.log(out.length + ":" + out[5].length + ":" + out[5][0]));
"#,
            &source,
            "await new Promise(resolve => setTimeout(resolve, 200));"
        ),
        "1024:1:5"
    );
}

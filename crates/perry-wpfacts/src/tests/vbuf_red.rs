//! VBUF1 red fixture (c09 R1+R2 first TOP after DEL-KEY+ENUM-DEL, three.module.js:5094 allocTexUnits,
//! `r = new Int32Array(n)`, rule viewedbuffer apply.rs:3173). `n` (= `v.length` of a uniform value)
//! may hold a program Proxy (perry-web dom/tokens.ts:159 element.style). Rule ViewedBuffer and rule
//! ShareBuffer have no Proxy arm: solve/proxy.rs PXESC escapes the Proxy and applies the rule to
//! [TOP] -> the view's [[ViewedArrayBuffer]] is TOP. ECMA-262: a Proxy exotic object has only the
//! [[ProxyHandler]]/[[ProxyTarget]] internal slots (10.5, Table 31); 23.2.5.1 TypedArray(object)
//! step 6.b tests [[TypedArrayName]]/[[ArrayBufferData]] on the argument itself (no trap runs), so a
//! Proxy argument is the "other object" case: AllocateTypedArray makes a fresh buffer
//! (23.2.5.1.4/23.2.5.1.5); its length/elements/@@iterator reads go through the traps (rule
//! `source`: Load/iterate, which have Proxy models). DataView(proxy) and %TypedArray%.prototype
//! methods on a Proxy receiver throw TypeError (RequireInternalSlot / ValidateTypedArray): no
//! buffer. Expected RED on base be688fccda: vbuf_proxy_*; GREEN: vbuf_plain_control.
use crate::{compute, FactsConfig};

fn first_block(src: &str) -> String {
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::topstop::capture_on();
    let _ = compute(&[("t.ts".to_string(), &hir)], &cfg);
    let v = crate::topstop::take_captured();
    v.into_iter()
        .next()
        .unwrap_or_else(|| "NO STOP".to_string())
}

fn assert_no_stop(src: &str) {
    let b = first_block(src);
    assert_eq!(b, "NO STOP", "{b}");
}

/// c09 shape: allocTexUnits(textures, n) with n = v.length, v a uniform value that may be a
/// program Proxy whose get trap hands back api members and the Proxy itself.
const ALLOC: &str = "const cacheI32: Int32Array[] = [];\n\
function allocTexUnits(n: any): Int32Array {\n\
  let r = cacheI32[n];\n\
  if (r === undefined) { r = new Int32Array(n); cacheI32[n] = r; }\n\
  for (let i = 0; i !== n; ++i) r[i] = i;\n\
  return r;\n\
}\n\
function setT1Array(v: any): number { const n = v.length; const u = allocTexUnits(n); return u.length; }\n";

/// RED on base: the Proxy reaches `new Int32Array(n)` through `v.length` (the get trap returns the
/// Proxy itself among the api values).
#[test]
fn vbuf_proxy_length_reaches_typed_array_ctor() {
    let src = format!(
        "{ALLOC}const api: any = {{ item: (i: number): string => String(i), len: 0 }};\n\
let me: any = null;\n\
me = new Proxy(api, {{ get(t: any, k: any) {{ if (k === \"self\") return me; if (typeof k === \"string\" && k in t) return t[k]; return me; }} }});\n\
const vals: any[] = [[1, 2, 3], me];\n\
const v = vals[process.argv.length > 9 ? 1 : 0];\n\
console.log(setT1Array(v));\n"
    );
    assert_no_stop(&src);
}

/// RED on base: a Proxy (or an array) as the direct TypedArray constructor argument; the view's
/// buffer is fresh either way.
#[test]
fn vbuf_proxy_typed_array_ctor_arg() {
    assert_no_stop(
        "const p: any = new Proxy({ length: 2, 0: 1, 1: 2 } as any, { get(o: any, k: any) { return o[k]; } });\n\
const xs: any[] = [[5, 6], p];\n\
const x = xs[process.argv.length > 9 ? 1 : 0];\n\
const ta = new Int32Array(x);\n\
console.log(ta.length, ta.buffer.byteLength);\n",
    );
}

/// RED on base: Proxy or ArrayBuffer as the argument: the buffer is the ArrayBuffer or a fresh one.
#[test]
fn vbuf_proxy_or_buffer_ctor_arg() {
    assert_no_stop(
        "const ab = new ArrayBuffer(8);\n\
const p: any = new Proxy({ length: 2 } as any, { get(o: any, k: any) { return o[k]; } });\n\
const xs: any[] = [ab, p];\n\
const x = xs[process.argv.length > 9 ? 1 : 0];\n\
const ta = new Uint8Array(x);\n\
console.log(ta.length, ta.buffer.byteLength);\n",
    );
}

/// Control (green on base): no Proxy.
#[test]
fn vbuf_plain_control() {
    let src = format!("{ALLOC}const vals: any[] = [[1, 2, 3], {{ length: 2 }}];\nconst v = vals[process.argv.length > 9 ? 1 : 0];\nconsole.log(setT1Array(v));\n");
    assert_no_stop(&src);
}

// #6303 — `String.prototype.replace(re, fn)` must invoke the replacer callback
// even when codegen cannot statically prove the search value is a RegExp AND the
// program links a `perry-ext-*` staticlib (here `http`).
//
// The ext archives bundle their own copy of perry-runtime and are linked BEFORE
// stdlib/runtime, so a copy built without `regex-engine` used to win the link for
// the unconditionally-exported dispatchers (`js_string_replace_search_dyn`,
// `js_native_call_method`) — whose RegExp detection is `#[cfg]`-ed out of the
// function BODY. The RegExp then got ToString-coerced and searched for literally,
// so the callback fired ZERO times and `replace` returned the subject unchanged.
//
// Both halves are required to reproduce:
//   1. `http` (any perry-ext-* lib that bundles perry-runtime) is linked, and
//   2. the regex reaches `.replace` through a value codegen can't type as a
//      RegExp (an object property / a module-level `var`), which routes through
//      `js_string_replace_search_dyn` instead of the statically-typed
//      `js_string_replace_regex_fn`.
//
// This is get-intrinsic's `stringToPath`, which is what blocked Express boot: it
// returned `[]`, so `'%' + '' + '%'` threw `SyntaxError: intrinsic %% does not exist!`.
import * as http from 'http';

// get-intrinsic's rePropName: carries a backreference (\2) and a lookahead, so it
// also exercises the fancy-regex fallback path.
const rePropName =
  /[^%.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|%$))/g;
const reSimple = /[^.]+/g;

// Read through an object property: codegen cannot statically type these as RegExp,
// so they lower to the runtime `searchValue` dispatcher (#4871).
const holder: { fancy: RegExp; simple: RegExp } = { fancy: rePropName, simple: reSimple };

function stringToPath(s: string): string[] {
  const out: string[] = [];
  s.replace(holder.fancy, function (m: string, num: string): string {
    out[out.length] = num || m;
    return '';
  } as any);
  return out;
}

let simpleCalls = 0;
const simpleOut = 'a.b.c'.replace(holder.simple, function (m: string): string {
  simpleCalls++;
  return m.toUpperCase();
} as any);

console.log('fancy dyn :', JSON.stringify(stringToPath('%String.prototype.indexOf%')));
console.log('simple dyn:', simpleOut, simpleCalls);

// Force the `perry-ext-http` staticlib — and therefore its bundled perry-runtime
// copy — into the link. Constructing the server is enough: it pulls
// `js_node_http_create_server_with_options` out of `libperry_ext_http.a`, which
// drags in that archive's perry-runtime codegen units. No `listen` needed (this
// test is about the link, not about serving).
const server = http.createServer(function (_req: any, res: any): void {
  res.end('ok');
});
console.log('server    :', typeof server.listen === 'function');

//! #6559: `new Function(p1, …, body)` with a RUNTIME-constructed body now
//! evaluates through the scoped interpreter (`perry-runtime/src/dyn_eval/`),
//! bridging into the real runtime in both directions.
//!
//! These fixtures are condensed from the ACTUAL code the three target
//! libraries generate (captured by hooking `Function` under Node 26 while
//! running ajv@8.20 / fast-json-stringify@7.0 / find-my-way@9.6):
//!   * ajv validator shape — scope consts, named function expression with
//!     `.errors` expando, destructured-default params, for/for-in, labeled
//!     break, host format fn + RegExp from scope, `key.replace(/~/g,…)`;
//!   * fast-json-stringify serializer shape — destructured serializer
//!     helpers, `.bind`, hoisted function decls calling each other, the
//!     `!addComma && (addComma = true) || (json += ',')` trick, template
//!     literal TypeError throw;
//!   * find-my-way matcher shapes — `new NullObject()` on a HOST class
//!     passed as a Function parameter, `charCodeAt` prefix matcher, `this`
//!     bound matcher methods, `Math.clz32` + `&=`, sloppy assignment to an
//!     undeclared name, host `Map.get` through `this`;
//!   * zod probe regression — `new Function("")` now succeeds, and a
//!     zod-JIT-shaped generated body (arrows, spread, `.map`) runs.
//!
//! The real-library end-to-end proof (npm-installed ajv / fjs / find-my-way)
//! lives in `issue_6559_real_libs_e2e.rs`.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        // The STATIC wrapper crates (not `-p perry-runtime`, which is
        // rlib-only since #5422): the compiled fixtures link the real
        // `libperry_runtime.a` / `libperry_stdlib.a`, and they must come from
        // THIS tree — a stale archive found elsewhere on the search path
        // would miss the dyn-eval symbols under test (the staticlib-shadow
        // trap).
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static")
            .output()
            .expect("run cargo build -p perry-runtime-static -p perry-stdlib-static");
        assert!(
            build.status.success(),
            "cargo build of the static archives failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

fn compile_and_run(fixture: &str) -> (String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), fixture).expect("write entry");
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile must succeed\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        run.status.success(),
        "binary must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    (stdout, stderr)
}

/// ajv compiled-validator shape (condensed from the real capture: scope
/// consts, `return function validate10(data, {instancePath="", rootData=
/// data}={}) …`, host format fn + RegExp from scope, for-in additional-
/// properties scan, labeled duplicate scan, `.errors` expando).
#[test]
fn ajv_validator_shape_end_to_end() {
    let fixture = r##"
declare function require(name: string): any;

function minLen(s: string): number { return s.length; }

// The body reaches the runtime as a RUNTIME string (built from parts), so
// codegen defers it to js_function_ctor_from_strings.
const parts: string[] = [
  'const schema = scope.schema;',
  'const fmt = scope.fmt;',
  'const pat = scope.pat;',
  'return function validate10(data, {instancePath="", rootData=data}={}) {',
  '  let vErrors = null;',
  '  let errors = 0;',
  '  if (data && typeof data == "object" && !Array.isArray(data)) {',
  '    if (data.name === undefined) {',
  '      const err0 = {instancePath, schemaPath: "#/required", keyword: "required", params: {missingProperty: "name"}, message: "must have required property \'" + "name" + "\'"};',
  '      if (vErrors === null) { vErrors = [err0]; } else { vErrors.push(err0); }',
  '      errors++;',
  '    }',
  '    if (data.name !== undefined) {',
  '      let data0 = data.name;',
  '      if (typeof data0 === "string") {',
  '        if (fmt(data0) < schema.minLength) {',
  '          const err2 = {instancePath: instancePath + "/name", keyword: "minLength", message: "must NOT have fewer than " + schema.minLength + " characters"};',
  '          if (vErrors === null) { vErrors = [err2]; } else { vErrors.push(err2); }',
  '          errors++;',
  '        }',
  '      } else {',
  '        const err3 = {instancePath: instancePath + "/name", keyword: "type", message: "must be string"};',
  '        if (vErrors === null) { vErrors = [err3]; } else { vErrors.push(err3); }',
  '        errors++;',
  '      }',
  '    }',
  '    if (data.zip !== undefined) {',
  '      let data7 = data.zip;',
  '      if (typeof data7 === "string") {',
  '        if (!pat.test(data7)) {',
  '          const err15 = {instancePath: instancePath + "/zip", keyword: "pattern", message: "must match pattern"};',
  '          if (vErrors === null) { vErrors = [err15]; } else { vErrors.push(err15); }',
  '          errors++;',
  '        }',
  '      }',
  '    }',
  '    for (const key0 in data) {',
  '      if (!((key0 === "name") || (key0 === "zip") || (key0 === "tags"))) {',
  '        const errA = {instancePath: instancePath + "/" + key0.replace(/~/g, "~0").replace(/\\//g, "~1"), keyword: "additionalProperties"};',
  '        if (vErrors === null) { vErrors = [errA]; } else { vErrors.push(errA); }',
  '        errors++;',
  '      }',
  '    }',
  '    if (data.tags !== undefined) {',
  '      let data3 = data.tags;',
  '      if (Array.isArray(data3)) {',
  '        let i1 = data3.length;',
  '        let j0;',
  '        if (i1 > 1) {',
  '          outer0:',
  '          for (; i1--;) {',
  '            for (j0 = i1; j0--;) {',
  '              if (data3[i1] === data3[j0]) {',
  '                const errD = {keyword: "uniqueItems", message: "must NOT have duplicate items (items ## " + j0 + " and " + i1 + " are identical)"};',
  '                if (vErrors === null) { vErrors = [errD]; } else { vErrors.push(errD); }',
  '                errors++;',
  '                break outer0;',
  '              }',
  '            }',
  '          }',
  '        }',
  '      }',
  '    }',
  '  } else {',
  '    const errT = {instancePath, keyword: "type", message: "must be object"};',
  '    if (vErrors === null) { vErrors = [errT]; } else { vErrors.push(errT); }',
  '    errors++;',
  '  }',
  '  validate10.errors = vErrors;',
  '  return errors === 0;',
  '}',
];
const sourceCode: string = parts.join("\n");

const scope: any = {
  schema: { minLength: 1 },
  fmt: minLen,
  pat: /^[0-9]{5}$/,
};

const makeValidate: any = new Function("self", "scope", sourceCode);
const validate: any = makeValidate(null, scope);

const good: any = { name: "Ada", zip: "12345", tags: ["a", "b"] };
// NOTE: `validate.errors === null` after a passing run is not asserted here —
// compiled dot-reads of a stored-null FUNCTION expando return a hole sentinel
// (pre-existing perry bug, reproduced WITHOUT the interpreter: a plain
// compiled `fn.errors = null; fn.errors === null` prints false). The error-
// ARRAY path below is what ajv consumers read and is asserted strictly.
console.log("good:", validate(good));

const bad: any = { name: "", zip: "abc", extra: 1, tags: ["x", "x"] };
const ok = validate(bad);
const errs: any[] = validate.errors;
console.log("bad:", ok, errs.length);
const kws: string[] = [];
for (const e of errs) { kws.push(e.keyword); }
kws.sort();
console.log("keywords:", kws.join(","));
console.log("dupmsg:", errs.filter((e: any) => e.keyword === "uniqueItems")[0].message);
"##;
    let (stdout, _stderr) = compile_and_run(fixture);
    assert!(stdout.contains("good: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("bad: false 4"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("keywords: additionalProperties,minLength,pattern,uniqueItems"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("dupmsg: must NOT have duplicate items (items ## 0 and 1 are identical)"),
        "stdout:\n{stdout}"
    );
}

/// fast-json-stringify serializer shape (condensed from the real capture).
#[test]
fn fast_json_stringify_shape_end_to_end() {
    let fixture = r##"
const serializer: any = {
  quote(s: string): string { return JSON.stringify(String(s)); },
  asString(s: any): string { return this.quote(s); },
  asNumber(n: any): string { return String(Number(n)); },
  asBoolean(b: any): string { return b ? "true" : "false"; },
  asInteger(n: any): string { return String(Math.trunc(Number(n))); },
};

const lines: string[] = [
  'const {',
  '  asString,',
  '  asNumber,',
  '  asBoolean',
  '} = serializer',
  'const asInteger = serializer.asInteger.bind(serializer)',
  "const JSON_STR_BEGIN_OBJECT = '{'",
  "const JSON_STR_END_OBJECT = '}'",
  "const JSON_STR_COMMA = ','",
  "const JSON_STR_EMPTY_ARRAY = '[]'",
  'function anonymous2 (obj) {',
  "  let json = ''",
  '  if (obj === null) return JSON_STR_EMPTY_ARRAY',
  '  if (!Array.isArray(obj)) {',
  '    throw new TypeError(`The value of \'#/properties/list\' does not match schema definition.`)',
  '  }',
  '  const arrayLength = obj.length',
  "  json += '['",
  '  if (arrayLength > 0) {',
  '    const value = obj[0]',
  '    json += asNumber(value)',
  '    for (let i = 1; i < arrayLength; i++) {',
  '      json += JSON_STR_COMMA',
  '      const value = obj[i]',
  '      json += asNumber(value)',
  '    }',
  '  }',
  "  return json + ']'",
  '}',
  'function anonymous0 (input) {',
  "  const obj = (input && typeof input.toJSON === 'function') ? input.toJSON() : input",
  '  if (obj === null) return JSON_STR_BEGIN_OBJECT + JSON_STR_END_OBJECT',
  "  let json = ''",
  '  json += JSON_STR_BEGIN_OBJECT',
  '  let addComma_7 = false',
  '  const value_id = obj["id"]',
  '  if (value_id !== undefined) {',
  '    !addComma_7 && (addComma_7 = true) || (json += JSON_STR_COMMA)',
  '    json += "\\"id\\":"',
  '    json += asInteger(value_id)',
  '  } else {',
  "    throw new Error('\"id\" is required!')",
  '  }',
  '  const value_name = obj["name"]',
  '  if (value_name !== undefined) {',
  '    !addComma_7 && (addComma_7 = true) || (json += JSON_STR_COMMA)',
  '    json += "\\"name\\":"',
  '    json += asString(value_name)',
  '  }',
  '  const value_active = obj["active"]',
  '  if (value_active !== undefined) {',
  '    !addComma_7 && (addComma_7 = true) || (json += JSON_STR_COMMA)',
  '    json += "\\"active\\":"',
  '    json += asBoolean(value_active)',
  '  }',
  '  const value_list = obj["list"]',
  '  if (value_list !== undefined) {',
  '    !addComma_7 && (addComma_7 = true) || (json += JSON_STR_COMMA)',
  '    json += "\\"list\\":"',
  '    json += anonymous2(value_list)',
  '  }',
  '  json += JSON_STR_END_OBJECT',
  '  return json',
  '}',
  'const main = anonymous0',
  'return main',
];
const code: string = lines.join("\n");

const build: any = new Function("validator", "serializer", code);
const stringify: any = build(null, serializer);

console.log("out:", stringify({ id: 7.9, name: 'x"y', active: true, list: [1, 2.5, 3] }));
try {
  stringify({ name: "missing id" });
  console.log("NO_THROW");
} catch (e: any) {
  console.log("caught:", e.message);
}
try {
  stringify({ id: 1, list: "not an array" });
  console.log("NO_THROW2");
} catch (e: any) {
  console.log("caught2:", e.message.indexOf("does not match schema definition") !== -1);
}
"##;
    let (stdout, _stderr) = compile_and_run(fixture);
    assert!(
        stdout.contains(r#"out: {"id":7,"name":"x\"y","active":true,"list":[1,2.5,3]}"#),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains(r#"caught: "id" is required!"#), "stdout:\n{stdout}");
    assert!(stdout.contains("caught2: true"), "stdout:\n{stdout}");
    assert!(!stdout.contains("NO_THROW"), "stdout:\n{stdout}");
}

/// find-my-way matcher shapes: `new NullObject()` on a host class received
/// as a Function parameter, charCodeAt prefix matcher, `this`-bound
/// constraint matchers with Math.clz32 / `&=` / sloppy `value = …`, host
/// Map through `this`.
#[test]
fn find_my_way_shapes_end_to_end() {
    let fixture = r##"
class NullObject {
}

// _createParamsObject codegen (verbatim shape).
const paramsLines: string[] = [
  'const fn = function _createParamsObject (paramsArray) {',
  'const params = new NullObject()',
  "params['id'] = paramsArray[0]",
  "params['bookId'] = paramsArray[1]",
  'return params',
  '}',
  'return fn',
];
const makeParams: any = (new Function("NullObject", paramsLines.join("\n")) as any)(NullObject);
const params: any = makeParams(["42", "9"]);
console.log("params:", params.id, params.bookId, params instanceof NullObject);

// matchPrefix codegen (verbatim): '/users' prefix scan.
const prefixSrc: string = ["return path.charCodeAt(i + 1) === 115 && path.charCodeAt(i + 2) === 101", ""].join("");
const matchPrefix: any = new Function("path", "i", prefixSrc);
console.log("prefix:", matchPrefix("/se/x", 0), matchPrefix("/xx/x", 0));

// getMatchingHandler codegen (verbatim shape incl. sloppy `value =`).
const handlerLines: string[] = [
  '    let candidates = 1',
  '    let mask, matches',
  '      mask = -2',
  '      value = derivedConstraints.version',
  '      if (value === undefined) {',
  '        candidates &= mask',
  '      } else {',
  '        matches = this.constrainedHandlerStores.version.get(value) || 0',
  '        candidates &= matches',
  '      }',
  '      if (candidates === 0) return null;',
  'return this.handlers[31 - Math.clz32(candidates)]',
];
const stores: any = { version: new Map<string, number>() };
stores.version.set("1.2.0", 1);
const node: any = {
  handlers: ["h0", "h1"],
  constrainedHandlerStores: stores,
  getMatchingHandler: new Function("derivedConstraints", handlerLines.join("\n")),
};
console.log("handler hit:", node.getMatchingHandler({ version: "1.2.0" }));
console.log("handler miss:", node.getMatchingHandler({ version: "9.9.9" }));

// deriveSyncConstraints codegen (verbatim).
const deriveSrc: string = [
  "return {",
  "   version: req.headers['accept-version'],",
  "   host: req.headers.host || req.headers[':authority'],",
  "}",
].join("\n");
const derive: any = new Function("req", "ctx", deriveSrc);
const derived: any = derive({ headers: { "accept-version": "1.2.0", host: "example.com" } }, null);
console.log("derived:", derived.version, derived.host);
"##;
    let (stdout, _stderr) = compile_and_run(fixture);
    assert!(stdout.contains("params: 42 9 true"), "stdout:\n{stdout}");
    assert!(stdout.contains("prefix: true false"), "stdout:\n{stdout}");
    assert!(stdout.contains("handler hit: h0"), "stdout:\n{stdout}");
    assert!(stdout.contains("handler miss: null"), "stdout:\n{stdout}");
    assert!(stdout.contains("derived: 1.2.0 example.com"), "stdout:\n{stdout}");
}

/// zod probe regression: the feature probe now SUCCEEDS (interpreter path)
/// and a zod-JIT-shaped body (arrows, spread, `.map`, `in`) runs correctly.
#[test]
fn zod_probe_and_jit_shape_end_to_end() {
    let fixture = r##"
// zod 4's JIT feature probe.
let jit = false;
try {
  const probeSrc: string = ["", ""].join("");
  new Function(probeSrc);
  jit = true;
} catch {
  jit = false;
}
console.log("probe:", jit);

// Condensed zod-JIT generated shape.
const lines: string[] = [
  '  const input = payload.value;',
  '  const newResult = {};',
  '  const key_0 = shape["name"]._zod.run({ value: input["name"], issues: [] }, ctx);',
  '  const key_0_present = "name" in input;',
  '  if (key_0.issues.length) {',
  '    payload.issues = payload.issues.concat(key_0.issues.map(iss => ({',
  '      ...iss,',
  '      path: iss.path ? ["name", ...iss.path] : ["name"]',
  '    })));',
  '  }',
  '  if (key_0_present) { newResult["name"] = key_0.value; }',
  '  payload.value = newResult;',
  '  return payload;',
];

const shape: any = {
  name: {
    _zod: {
      run(payload: any, ctx: any): any {
        if (typeof payload.value !== "string") {
          return { value: payload.value, issues: [{ code: "invalid_type", path: null }] };
        }
        return { value: payload.value, issues: [] };
      },
    },
  },
};

const parse: any = new Function("shape", "payload", "ctx", lines.join("\n"));

const okPayload: any = parse(shape, { value: { name: "ada" }, issues: [] }, {});
console.log("ok:", okPayload.value.name, okPayload.issues.length);

const badPayload: any = parse(shape, { value: { name: 42 }, issues: [] }, {});
console.log("bad:", badPayload.issues.length, badPayload.issues[0].path.join("/"), badPayload.issues[0].code);
"##;
    let (stdout, _stderr) = compile_and_run(fixture);
    assert!(stdout.contains("probe: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("ok: ada 0"), "stdout:\n{stdout}");
    assert!(stdout.contains("bad: 1 name invalid_type"), "stdout:\n{stdout}");
}

/// Unsupported constructs throw the named diagnostic — catchable, no crash.
#[test]
fn unsupported_construct_diagnostic_end_to_end() {
    let fixture = r##"
const src: string = ["return class", " {}"].join("");
try {
  const f: any = new Function(src);
  f();
  console.log("NO_THROW");
} catch (e: any) {
  const msg: string = String(e.message);
  console.log("caught:", msg.indexOf("unsupported construct") !== -1, msg.indexOf("class") !== -1);
}
"##;
    let (stdout, _stderr) = compile_and_run(fixture);
    assert!(stdout.contains("caught: true true"), "stdout:\n{stdout}");
    assert!(!stdout.contains("NO_THROW"), "stdout:\n{stdout}");
}

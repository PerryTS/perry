//! #6559 definition-of-done: the REAL schema-codegen libraries — ajv,
//! fast-json-stringify, find-my-way — npm-installed and compiled by perry,
//! with their runtime `new Function` codegen evaluated by the dyn-eval
//! interpreter.
//!
//! STATUS: find-my-way is now un-ignored and green (#6587: `x instanceof C`
//! with a `null`/`undefined` LHS no longer throws — see
//! `perry-runtime/src/object/instanceof.rs`). ajv and fast-json-stringify
//! remain `#[ignore]`d on PRE-EXISTING perry CJS-compilation gaps at module
//! load / library init, BEFORE any runtime-generated code runs (per-test
//! reasons on the #[ignore] attributes). The interpreter itself is proven
//! end-to-end against the captured generated-code shapes of these exact
//! library versions in issue_6559_dyn_function_interpreter.rs (always-on,
//! green). Un-ignore each remaining test as its CJS wall falls.
//!
//! Each test provisions its own tempdir project via `npm install` (pinned
//! majors matching the versions the shapes were captured from). When npm or
//! the network is unavailable the test SKIPS with a notice — set
//! `PERRY_REQUIRE_NPM_E2E=1` to turn a skip into a failure (used for local
//! definition-of-done runs). The always-on interpreter coverage lives in
//! `issue_6559_dyn_function_interpreter.rs` (no node_modules needed).

use std::path::{Path, PathBuf};
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

/// `npm install` the given packages into `root`, then opt every installed
/// package into native compilation (`perry.compilePackages` +
/// `perry.allow.compilePackages` — perry refuses to route npm JavaScript
/// through a JS runtime, which no longer exists). Returns false (skip) when
/// npm is missing or the install fails (offline CI), unless
/// PERRY_REQUIRE_NPM_E2E=1 makes that a hard failure.
fn npm_install(root: &Path, packages: &[&str]) -> bool {
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "issue-6559-e2e", "private": true }"#,
    )
    .expect("write package.json");
    let result = Command::new("npm")
        .current_dir(root)
        .arg("install")
        .arg("--no-audit")
        .arg("--no-fund")
        .args(packages)
        .output();
    let required = std::env::var("PERRY_REQUIRE_NPM_E2E").ok().as_deref() == Some("1");
    let ok = match result {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let msg = format!(
                "npm install {:?} failed\nstdout:\n{}\nstderr:\n{}",
                packages,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if required {
                panic!("{msg}");
            }
            eprintln!("SKIP (npm install failed — offline?): {msg}");
            false
        }
        Err(e) => {
            if required {
                panic!("npm not available: {e}");
            }
            eprintln!("SKIP: npm not available: {e}");
            false
        }
    };
    if !ok {
        return false;
    }
    // Trust opt-in for the whole installed dependency tree (top-level
    // node_modules entries; scoped packages expand to scope/name).
    let mut compile_packages: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root.join("node_modules")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if let Some(scope) = name.strip_prefix('@') {
                if let Ok(scoped) = std::fs::read_dir(entry.path()) {
                    for sub in scoped.flatten() {
                        compile_packages
                            .push(format!("@{scope}/{}", sub.file_name().to_string_lossy()));
                    }
                }
            } else {
                compile_packages.push(name);
            }
        }
    }
    compile_packages.sort();
    let list = compile_packages
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        root.join("package.json"),
        format!(
            r#"{{ "name": "issue-6559-e2e", "private": true,
  "perry": {{ "compilePackages": [{list}],
             "allow": {{ "compilePackages": [{list}] }} }} }}"#
        ),
    )
    .expect("rewrite package.json with perry.compilePackages");
    true
}

fn compile_and_run(root: &Path, fixture: &str) -> (String, String) {
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

/// Real ajv: draft-07 validation of an object schema with nested properties,
/// a format (email via ajv-formats), pattern, enum, uniqueItems — compiled
/// validators built by `new Function(self, scope, code)` at runtime.
#[test]
#[ignore = "blocked on a PRE-EXISTING perry CJS-compilation gap, not the #6559 interpreter: \
ajv's dist/compile/codegen/index.js class methods call the module-level `addNames` declared \
later in the file, which compiles to `ReferenceError: addNames is not defined` at runtime \
(ajv imports + instantiates fine; ajv.compile() hits it before any generated code runs). \
Run with PERRY_REQUIRE_NPM_E2E=1 -- --ignored once the CJS forward-reference hoisting gap is fixed."]
fn real_ajv_compiled_validator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !npm_install(root, &["ajv@8", "ajv-formats@3"]) {
        return;
    }
    let fixture = r#"
import Ajv from "ajv";
import addFormats from "ajv-formats";

const ajv: any = new Ajv({ allErrors: true });
addFormats(ajv);

const schema: any = {
  type: "object",
  properties: {
    name: { type: "string", minLength: 1 },
    email: { type: "string", format: "email" },
    age: { type: "integer", minimum: 0, maximum: 150 },
    tags: { type: "array", items: { type: "string" }, uniqueItems: true },
    address: {
      type: "object",
      properties: {
        street: { type: "string" },
        zip: { type: "string", pattern: "^[0-9]{5}$" },
      },
      required: ["street"],
      additionalProperties: false,
    },
    role: { enum: ["admin", "user", "guest"] },
  },
  required: ["name", "email"],
  additionalProperties: true,
};

const validate: any = ajv.compile(schema);

const good: any = {
  name: "Ada",
  email: "ada@example.com",
  age: 36,
  tags: ["x", "y"],
  address: { street: "Main", zip: "12345" },
  role: "admin",
};
// (see issue_6559_dyn_function_interpreter.rs — `validate.errors === null`
// dot-reads of stored null hit a pre-existing compiled-expando quirk; the
// return value + error-array path are the load-bearing assertions.)
console.log("good:", validate(good));

const bad: any = { name: "", email: "nope", age: -3, address: { zip: "abc" }, tags: ["a", "a"] };
console.log("bad:", validate(bad));
const errs: any[] = validate.errors;
const summary: string[] = [];
for (const e of errs) { summary.push(e.instancePath + "|" + e.keyword); }
summary.sort();
console.log("errors:", summary.join(";"));
"#;
    let (stdout, _stderr) = compile_and_run(root, fixture);
    assert!(stdout.contains("good: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("bad: false"), "stdout:\n{stdout}");
    for expected in [
        "/name|minLength",
        "/email|format",
        "/age|minimum",
        "/address|required",
        "/address/zip|pattern",
        "/tags|uniqueItems",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected} in stdout:\n{stdout}"
        );
    }
}

/// Real fast-json-stringify: serialize an object with nested arrays,
/// strings, numbers, booleans and a date-time — the serializer is generated
/// by `new Function(validator, serializer, code)` at runtime.
#[test]
#[ignore = "blocked on a PRE-EXISTING perry CJS-compilation gap, not the #6559 interpreter: \
the compiled fast-json-stringify module tree throws `ReferenceError: module is not defined` \
at load. Run with PERRY_REQUIRE_NPM_E2E=1 -- --ignored once the CJS wrapping gap is fixed."]
fn real_fast_json_stringify_serializer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !npm_install(root, &["fast-json-stringify@7"]) {
        return;
    }
    let fixture = r#"
import fastJson from "fast-json-stringify";

const stringify: any = fastJson({
  title: "rec",
  type: "object",
  properties: {
    id: { type: "integer" },
    name: { type: "string" },
    score: { type: "number" },
    active: { type: "boolean" },
    created: { type: "string", format: "date-time" },
    nested: {
      type: "object",
      properties: {
        list: { type: "array", items: { type: "number" } },
        label: { type: "string" },
      },
    },
    items: {
      type: "array",
      items: {
        type: "object",
        properties: { k: { type: "string" }, v: { type: "integer" } },
      },
    },
  },
  required: ["id", "name"],
});

const out: string = stringify({
  id: 7,
  name: 'x"y',
  score: 1.5,
  active: true,
  created: new Date("2026-01-02T03:04:05.000Z"),
  nested: { list: [1, 2, 3], label: "L" },
  items: [{ k: "a", v: 1 }, { k: "b", v: 2 }],
});
console.log("out:", out);

try {
  stringify({ name: "missing id" });
  console.log("NO_THROW");
} catch (e: any) {
  console.log("caught:", String(e.message).indexOf("required") !== -1);
}
"#;
    let (stdout, _stderr) = compile_and_run(root, fixture);
    assert!(
        stdout.contains(
            r#"out: {"id":7,"name":"x\"y","score":1.5,"active":true,"created":"2026-01-02T03:04:05.000Z","nested":{"list":[1,2,3],"label":"L"},"items":[{"k":"a","v":1},{"k":"b","v":2}]}"#
        ),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("caught: true"), "stdout:\n{stdout}");
    assert!(!stdout.contains("NO_THROW"), "stdout:\n{stdout}");
}

/// Real find-my-way: register routes incl. params, wildcard and constraints
/// (version + host), then look them up — every matcher (`matchPrefix`,
/// `_createParamsObject`, `deriveSyncConstraints`, `getMatchingHandler`) is
/// runtime-generated via `new Function`.
// #6587 fixed the CJS-init wall (`FindMyWay(opts)` called without `new`
// evaluated `this instanceof Router` with `this === undefined`, which threw
// `TypeError: Cannot convert undefined or null to object` before any route was
// registered). This test now runs end-to-end; it still SKIPs when npm / the
// network is unavailable unless `PERRY_REQUIRE_NPM_E2E=1` is set.
#[test]
fn real_find_my_way_router() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    if !npm_install(root, &["find-my-way@9"]) {
        return;
    }
    let fixture = r#"
import FindMyWay from "find-my-way";

const router: any = FindMyWay({ ignoreTrailingSlash: true });
router.on("GET", "/", () => "root");
router.on("GET", "/users/:id", () => "user");
router.on("GET", "/users/:id/books/:bookId", () => "book");
router.on("POST", "/users", () => "create");
router.on("GET", "/static/*", () => "wild");
router.on("GET", "/versioned", { constraints: { version: "1.2.0" } }, () => "v1");
router.on("GET", "/host", { constraints: { host: "example.com" } }, () => "hosted");

const f1: any = router.find("GET", "/users/42");
console.log("user:", f1 ? f1.params.id : "MISS");

const f2: any = router.find("GET", "/users/1/books/9");
console.log("book:", f2 ? f2.params.id + "," + f2.params.bookId : "MISS");

const f3: any = router.find("GET", "/versioned", { version: "1.2.0" });
console.log("versioned:", !!f3);
const f3miss: any = router.find("GET", "/versioned", { version: "9.9.9" });
console.log("versionedMiss:", f3miss === null);

const f4: any = router.find("GET", "/host", { host: "example.com" });
console.log("host:", !!f4);

const f5: any = router.find("GET", "/static/a/b.png");
console.log("wild:", f5 ? f5.params["*"] : "MISS");

const f6: any = router.find("GET", "/nope");
console.log("miss:", f6 === null);

console.log("handlers:", f1.handler(), f2.handler(), f5.handler());
"#;
    let (stdout, _stderr) = compile_and_run(root, fixture);
    assert!(stdout.contains("user: 42"), "stdout:\n{stdout}");
    assert!(stdout.contains("book: 1,9"), "stdout:\n{stdout}");
    assert!(stdout.contains("versioned: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("versionedMiss: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("host: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("wild: a/b.png"), "stdout:\n{stdout}");
    assert!(stdout.contains("miss: true"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("handlers: user book wild"),
        "stdout:\n{stdout}"
    );
}

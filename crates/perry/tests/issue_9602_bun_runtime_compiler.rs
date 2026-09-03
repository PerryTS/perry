//! End-to-end regression coverage for #9602. The native `"bun"` module
//! exposes Bun's runtime transpiler plus the in-memory build/plugin subset
//! used by runtime hook modules.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn bun_transpiler_and_build_plugins_compile_and_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import { Transpiler, build } from "bun";

async function main() {
  const ts = new Transpiler({ loader: "ts" });
  const js = ts.transformSync("const n: number = 1; export { n }");
  console.log(!js.includes(": number"), js.includes("export { n }"));

  const asyncJs = await ts.transform("const value: string = 'ok'");
  console.log(!asyncJs.includes(": string"));

  const repl = new Transpiler({ loader: "js", replMode: true });
  console.log(JSON.stringify(repl.scanImports('import x from "pkg"; require("./cjs")')));
  console.log(JSON.stringify(ts.scan("export const answer: number = 42")));

  await Bun.write(
    "./entry.ts",
    'import host from "host-api"; import answer from "virtual:answer"; const n: number = answer; console.log(host, n);',
  );
  const result = await build({
    entrypoints: ["./entry.ts"],
    target: "bun",
    format: "esm",
    minify: false,
    external: ["host-api"],
    plugins: [{
      name: "virtual",
      setup(builder: any) {
        builder.onResolve({ filter: /^virtual:/ }, (args: any) => ({
          path: args.path,
          namespace: "v",
        }));
        builder.onLoad({ filter: /.*/, namespace: "v" }, () => ({
          contents: "export default 42",
          loader: "js",
        }));
      },
    }],
  });
  const bundled = await result.outputs[0].text();
  console.log(result.success, result.outputs.length, result.logs.length);
  console.log(bundled.includes("42"), !bundled.includes(": number"), bundled.includes("host-api"));

  await Bun.write("./invalid.ts", "const broken: = 1");
  const failed = await build({
    entrypoints: ["./invalid.ts"],
    target: "bun",
    format: "esm",
  });
  console.log(
    failed.success,
    failed.outputs.length,
    failed.logs.length,
    failed.logs[0].position.file.endsWith("invalid.ts"),
    failed.logs[0].position.line,
  );
}
main();
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "true true\ntrue\n[{\"path\":\"pkg\",\"kind\":\"import-statement\"},{\"path\":\"./cjs\",\"kind\":\"require-call\"}]\n{\"exports\":[\"answer\"],\"imports\":[]}\ntrue 1 0\ntrue true true\nfalse 0 1 true 1\n"
    );
}

//! #9600 — Bun's compact CLI utility shim pack, exercised through a compiled
//! standalone binary. Expected values are from Bun 1.3.14.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
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
    let run = Command::new(output)
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
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn bun_cli_utility_oracle_and_edge_cases() {
    let stdout = compile_and_run(
        r#"
import {
  YAML, TOML, semver, deepEquals, stripANSI, wrapAnsi, which,
  zstdDecompress, zstdDecompressSync, hash, JSONL, gc,
  generateHeapSnapshot,
} from "bun";

async function main() {
  console.log(JSON.stringify(YAML.parse("a: 1\nb:\n  - x")));
  console.log(JSON.stringify(TOML.parse('a = 1\n[b]\nc = "x"')));
  console.log(semver.order("1.0.0", "2.0.0"), semver.satisfies("1.2.3", "^1.0.0"));
  try { semver.order("bad", "1.0.0"); } catch (error: any) {
    console.log(error.name, error.message.trim());
  }
  const semverCases = [
    ["1.2.3", "1.2.3"], ["v1.2.3", "^1.0.0"],
    ["1.2.5", "~1.2.3"], ["1.3.0", "~1.2.3"],
    ["0.2.5", "^0.2.3"], ["0.3.0", "^0.2.3"],
    ["1.5.0", "1.2.3 - 2.0.0"], ["2.1.0", "1.2.3 - 2.0.0"],
    ["1.4.9", "1.4.x"], ["1.5.0", "1.4.x"],
    ["2.0.0", "<1 || >=2"], ["1.5.0", "<1 || >=2"],
    ["1.2.3-beta.1", "^1.2.3"],
    ["1.2.3-beta.2", ">=1.2.3-beta.1 <1.2.3"],
    ["1.2.3", ""], ["1.2.3", "latest"],
  ];
  console.log(JSON.stringify(semverCases.map(([version, range]) => semver.satisfies(version, range))));
  console.log(deepEquals({ a: [1, 2] }, { a: [1, 2] }));
  console.log(JSON.stringify(stripANSI("\x1b[31ma\x1b[0mb")));
  console.log(JSON.stringify(wrapAnsi("one two three", 7)));
  console.log(which("definitely-not-a-real-command-9600"));
  console.log(hash.xxHash64("abc").toString());

  const compressed = new Uint8Array([40,181,47,253,4,88,81,0,0,104,101,108,108,111,32,122,115,116,100,207,219,96,156]);
  console.log(new TextDecoder().decode(zstdDecompressSync(compressed)));
  console.log(new TextDecoder().decode(await zstdDecompress(compressed)));

  const parsed = JSONL.parseChunk('{"a":1}\n{"b":2}');
  console.log(JSON.stringify({ values: parsed.values, read: parsed.read, done: parsed.done, error: parsed.error }));
  const incomplete = JSONL.parseChunk('{"a":1}\n{"b":');
  console.log(JSON.stringify({ values: incomplete.values, read: incomplete.read, done: incomplete.done, error: incomplete.error }));
  const invalid = JSONL.parseChunk('{"a":1}\nnope\n');
  console.log(JSON.stringify({ values: invalid.values, read: invalid.read, done: invalid.done, error: invalid.error?.name }));

  const alias = YAML.parse("a: &shared\n  x: 1\nb: *shared");
  console.log(alias.a === alias.b, JSON.stringify(alias));
  const cycle = YAML.parse("root: &root\n  self: *root");
  console.log(cycle.root === cycle.root.self);
  console.log(JSON.stringify(YAML.parse("---\na: 1\n---\nb: 2")));
  const roundTrip = YAML.parse(YAML.stringify({ a: 1, b: ["x"] }, null, 2));
  console.log(JSON.stringify(roundTrip));
  try { YAML.parse("a: ["); } catch (error: any) { console.log(error.name); }

  console.log(gc() === undefined);
  try { generateHeapSnapshot(); } catch (error: any) {
    console.log(error.name, error.message.includes("v8"));
  }
}
main();
"#,
    );
    assert_eq!(
        stdout,
        concat!(
            "{\"a\":1,\"b\":[\"x\"]}\n",
            "{\"a\":1,\"b\":{\"c\":\"x\"}}\n",
            "-1 true\n",
            "Error Invalid SemVer: bad\n",
            "[true,true,true,false,true,false,true,false,true,false,true,false,false,true,true,true]\n",
            "true\n",
            "\"ab\"\n",
            "\"one two\\nthree\"\n",
            "null\n",
            "4952883123889572249\n",
            "hello zstd\n",
            "hello zstd\n",
            "{\"values\":[{\"a\":1},{\"b\":2}],\"read\":15,\"done\":true,\"error\":null}\n",
            "{\"values\":[{\"a\":1}],\"read\":7,\"done\":false,\"error\":null}\n",
            "{\"values\":[{\"a\":1}],\"read\":7,\"done\":false,\"error\":\"SyntaxError\"}\n",
            "true {\"a\":{\"x\":1},\"b\":{\"x\":1}}\n",
            "true\n",
            "[{\"a\":1},{\"b\":2}]\n",
            "{\"a\":1,\"b\":[\"x\"]}\n",
            "SyntaxError\n",
            "true\n",
            "TypeError true\n",
        )
    );
}

//! Regression coverage for #8751: Stripe's CommonJS request helper requires
//! `qs` and calls `qs.stringify` with indexed arrays and a Date serializer.
//! Compiling upstream qs pulls in get-intrinsic's legacy ES-shims chain, which
//! is hostile to Perry's AOT path. The bundled binding must win even when a
//! deliberately broken on-disk qs package is present transitively.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn stripe_style_dependency_uses_native_qs_without_compiling_installed_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-8751",
  "type": "module",
  "perry": {
    "compilePackages": ["stripe-fixture"],
    "allow": { "compilePackages": ["stripe-fixture"] }
  }
}"#,
    )
    .expect("write app package.json");

    let stripe = root.join("node_modules").join("stripe-fixture");
    std::fs::create_dir_all(&stripe).expect("mkdir stripe fixture");
    std::fs::write(
        stripe.join("package.json"),
        r#"{ "name": "stripe-fixture", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write stripe fixture package.json");
    std::fs::write(
        stripe.join("index.js"),
        r#"'use strict';
const qs = require('qs');

exports.encodeStripePayload = function encodeStripePayload(data) {
  return qs.stringify(data, {
    serializeDate: function serializeDate(date) {
      return Math.floor(date.getTime() / 1000).toString();
    },
    arrayFormat: 'indices'
  }).replace(/%5B/g, '[').replace(/%5D/g, ']');
};
"#,
    )
    .expect("write stripe fixture source");

    // If module resolution ever falls back to compiling the installed qs,
    // compilation or startup fails with this sentinel. A get-intrinsic stub is
    // included to retain the transitive shape reported in #8751.
    let qs = root.join("node_modules").join("qs");
    std::fs::create_dir_all(&qs).expect("mkdir hostile qs");
    std::fs::write(
        qs.join("package.json"),
        r#"{ "name": "qs", "version": "6.15.3", "main": "index.js" }"#,
    )
    .expect("write hostile qs package.json");
    std::fs::write(
        qs.join("index.js"),
        "throw new Error('AOT-HOSTILE-QS-SOURCE-WAS-COMPILED');\n",
    )
    .expect("write hostile qs source");
    let intrinsic = root.join("node_modules").join("get-intrinsic");
    std::fs::create_dir_all(&intrinsic).expect("mkdir get-intrinsic");
    std::fs::write(
        intrinsic.join("package.json"),
        r#"{ "name": "get-intrinsic", "version": "1.3.0", "main": "index.js" }"#,
    )
    .expect("write get-intrinsic package.json");
    std::fs::write(
        intrinsic.join("index.js"),
        "throw new SyntaxError('intrinsic %% does not exist!');\n",
    )
    .expect("write get-intrinsic source");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import qsDefault from "qs";
import * as qsNamespace from "qs";
import { parse, stringify } from "qs";
import { encodeStripePayload } from "stripe-fixture";

const payload = {
  customer: { name: "Ada Lovelace" },
  items: [
    { price: "p_1", quantity: 2 },
    { price: "p_2", quantity: 1 }
  ],
  metadata: { empty: null },
  created: new Date("2024-01-02T03:04:05Z")
};

console.log(encodeStripePayload(payload));
console.log(JSON.stringify(parse("customer[name]=Ada%20Lovelace&items[0][price]=p_1&items[1][price]=p_2&tag=a&tag=b")));
console.log(stringify({ a: ["x", "y"], empty: [], nil: null }, {
  arrayFormat: "brackets",
  allowEmptyArrays: true,
  strictNullHandling: true,
  addQueryPrefix: true
}));
console.log(stringify({ z: "last", a: "first" }, {
  sort: (left: string, right: string) => left < right ? -1 : left > right ? 1 : 0,
  encoder: (value: any) => "X" + String(value)
}));
console.log(qsNamespace.stringify({ a: 1 }), qsDefault.stringify({ a: 1 }), stringify({ a: 1 }));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "customer[name]=Ada%20Lovelace&items[0][price]=p_1&items[0][quantity]=2&items[1][price]=p_2&items[1][quantity]=1&metadata[empty]=&created=1704164645\n",
            "{\"customer\":{\"name\":\"Ada Lovelace\"},\"items\":[{\"price\":\"p_1\"},{\"price\":\"p_2\"}],\"tag\":[\"a\",\"b\"]}\n",
            "?a%5B%5D=x&a%5B%5D=y&empty[]&nil\n",
            "Xa=Xfirst&Xz=Xlast\n",
            "a=1 a=1 a=1\n"
        )
    );
}

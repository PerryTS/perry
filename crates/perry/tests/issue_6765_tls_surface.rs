//! Regression coverage for the first #6765 TLS surface increment.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn tls_exposes_extended_socket_and_alpn_helpers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import tls from "node:tls";

console.log(
  "surface:",
  typeof tls.convertALPNProtocols,
  typeof tls.TLSSocket.prototype.setKeyCert,
  typeof tls.TLSSocket.prototype.getSharedSigalgs,
  typeof tls.TLSSocket.prototype.getX509Certificate,
  typeof tls.TLSSocket.prototype.getPeerX509Certificate,
);
console.log(
  "lengths:",
  tls.TLSSocket.prototype.setKeyCert.length,
  tls.TLSSocket.prototype.getSharedSigalgs.length,
  tls.TLSSocket.prototype.getX509Certificate.length,
  tls.TLSSocket.prototype.getPeerX509Certificate.length,
);
for (const constructor of [tls.Server, tls.TLSSocket]) {
  const descriptor = Object.getOwnPropertyDescriptor(constructor, "prototype")!;
  console.log(
    "prototype descriptor:",
    descriptor.writable,
    descriptor.enumerable,
    descriptor.configurable,
  );
}

const out: any = {};
tls.convertALPNProtocols(["h2", "http/1.1"], out);
console.log("array:", Buffer.isBuffer(out.ALPNProtocols), out.ALPNProtocols.toString("hex"));

const source = Buffer.from([9, 2, 104, 50, 9]);
const copied: any = {};
tls.convertALPNProtocols(source.subarray(1, 4), copied);
source[2] = 120;
console.log("copy:", copied.ALPNProtocols.toString("hex"));

try {
  tls.convertALPNProtocols(["a".repeat(256)], {});
} catch (error: any) {
  console.log("range:", error instanceof RangeError, error.code);
}
"#,
    )
    .expect("write fixture");

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
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "surface: function function function function function\n",
            "lengths: 1 0 0 0\n",
            "prototype descriptor: true false false\n",
            "prototype descriptor: true false false\n",
            "array: true 02683208687474702f312e31\n",
            "copy: 026832\n",
            "range: true ERR_OUT_OF_RANGE\n",
        )
    );
}

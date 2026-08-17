//! Regression coverage for Node-compatible TLS server identity matching (#6765).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn tls_check_server_identity_matches_node_patterns_and_error_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
import tls from "node:tls";

function ok(host: any, cert: any) {
  return tls.checkServerIdentity(host, cert) === undefined;
}
function code(host: any, cert: any) {
  return (tls.checkServerIdentity(host, cert) as any)?.code;
}

console.log("numeric:", code(123, { subject: { CN: "123" } }));
console.log("trailing:", ok("a.example", { subject: { CN: "A.EXAMPLE." } }));
console.log("partial:", ok("a-cb.a.com", { subject: { CN: "*b.a.com" } }));
console.log("top-level:", code("a.com", { subject: { CN: "*.com" } }));
console.log("multiple:", ok("second.example", { subject: { CN: ["first.example", "second.example"] } }));
console.log("unicode separator:", code("foo。bar.example.com", { subject: { CN: "*.example.com" } }));

const ipError: any = tls.checkServerIdentity("127.0.0.1", {
  subject: { CN: "127.0.0.1" },
});
console.log("ip cn:", ipError.code, ipError.reason);

const cert = { subjectaltname: "DNS:good.example" };
const error: any = tls.checkServerIdentity("bad.example", cert);
console.log("error:", error instanceof Error, error.name, Object.keys(error).sort().join(","));
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
            "numeric: ERR_TLS_CERT_ALTNAME_INVALID\n",
            "trailing: true\n",
            "partial: true\n",
            "top-level: ERR_TLS_CERT_ALTNAME_INVALID\n",
            "multiple: true\n",
            "unicode separator: ERR_TLS_CERT_ALTNAME_INVALID\n",
            "ip cn: ERR_TLS_CERT_ALTNAME_INVALID IP: 127.0.0.1 is not in the cert's list: \n",
            "error: true Error cert,code,host,reason\n",
        )
    );
}

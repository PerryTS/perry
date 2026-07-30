//! End-to-end acceptance test: build a CA + leaf exactly the way Socket
//! Firewall's TLS-MITM path does (via the crate's PKI core), then verify
//! the chain with the real `openssl` CLI. This is the acceptance bar for
//! the wrapper's fidelity — a cert real TLS clients accept.
//!
//! Set `PERRY_SKIP_OPENSSL_E2E=1` to explicitly skip when OpenSSL is not
//! available in an intentionally minimal environment.

use std::io::Write;
use std::process::Command;

use perry_ext_node_forge::crypto::*;

fn openssl_available() -> bool {
    Command::new("openssl")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ca_attrs() -> Vec<Attr> {
    vec![
        Attr {
            key: "commonName".into(),
            value: "Socket Security CA".into(),
            value_tag: None,
        },
        Attr {
            key: "organizationName".into(),
            value: "Socket Security".into(),
            value_tag: None,
        },
    ]
}

#[test]
fn ca_and_leaf_verify_with_openssl() {
    if !openssl_available() {
        if std::env::var("PERRY_SKIP_OPENSSL_E2E").as_deref() == Ok("1") {
            eprintln!("PERRY_SKIP_OPENSSL_E2E=1 — skipping OpenSSL verification");
            return;
        }
        panic!("openssl not found on PATH (set PERRY_SKIP_OPENSSL_E2E=1 to explicitly skip)");
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // ── CA (mirrors src/lib/util/genCaKeyPair.ts) ──────────────────
    let (ca_priv_pem, ca_pub_pem) = generate_key_pair(2048).unwrap();
    let ca_spec = CertSpec {
        public_key_pem: ca_pub_pem,
        serial_hex: "01".into(),
        not_before_unix: now - 60,
        not_after_unix: now + 2 * 365 * 24 * 60 * 60,
        subject: ca_attrs(),
        issuer: ca_attrs(),
        extensions: ExtSet {
            basic_constraints: Some(BasicConstraintsSpec {
                ca: true,
                critical: true,
            }),
            key_usage: Some(KeyUsageSpec {
                key_cert_sign: true,
                crl_sign: true,
                critical: true,
                ..Default::default()
            }),
            subject_key_identifier: true,
            ..Default::default()
        },
    };
    let ca_pem = build_and_sign(&ca_spec, &ca_priv_pem).unwrap();

    // ── Leaf (mirrors src/lib/firewall/cert/host.ts) ───────────────
    // Issuer is derived from parsing the CA's subject — the sfw path.
    let ca_subject_attrs = cert_subject_attrs(&ca_pem).unwrap();
    let (_leaf_priv_pem, leaf_pub_pem) = generate_key_pair(2048).unwrap();
    let leaf_spec = CertSpec {
        public_key_pem: leaf_pub_pem,
        serial_hex: "02".into(),
        not_before_unix: now - 60,
        not_after_unix: now + 365 * 24 * 60 * 60,
        subject: vec![Attr {
            key: "commonName".into(),
            value: "example.com".into(),
            value_tag: None,
        }],
        issuer: ca_subject_attrs,
        extensions: ExtSet {
            basic_constraints: Some(BasicConstraintsSpec {
                ca: false,
                critical: false,
            }),
            key_usage: Some(KeyUsageSpec {
                digital_signature: true,
                key_encipherment: true,
                ..Default::default()
            }),
            ext_key_usage: Some(ExtKeyUsageSpec {
                server_auth: true,
                ..Default::default()
            }),
            subject_alt_names: vec!["example.com".into(), "www.example.com".into()],
            ..Default::default()
        },
    };
    // Signed by the CA's PRIVATE key.
    let leaf_pem = build_and_sign(&leaf_spec, &ca_priv_pem).unwrap();

    let dir = std::env::temp_dir().join("perry_node_forge_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let ca_path = dir.join("ca.pem");
    let leaf_path = dir.join("leaf.pem");
    std::fs::File::create(&ca_path)
        .unwrap()
        .write_all(ca_pem.as_bytes())
        .unwrap();
    std::fs::File::create(&leaf_path)
        .unwrap()
        .write_all(leaf_pem.as_bytes())
        .unwrap();

    // openssl verify -CAfile ca.pem leaf.pem
    let verify = Command::new("openssl")
        .arg("verify")
        .arg("-CAfile")
        .arg(&ca_path)
        .arg(&leaf_path)
        .output()
        .unwrap();
    let vout = String::from_utf8_lossy(&verify.stdout);
    let verr = String::from_utf8_lossy(&verify.stderr);
    println!("openssl verify stdout: {vout}");
    println!("openssl verify stderr: {verr}");
    assert!(
        verify.status.success() && vout.contains(": OK"),
        "openssl verify failed: {vout}{verr}"
    );

    // openssl x509 -in leaf.pem -text — SANs + extensions visible.
    let text = Command::new("openssl")
        .arg("x509")
        .arg("-in")
        .arg(&leaf_path)
        .arg("-text")
        .arg("-noout")
        .output()
        .unwrap();
    let tout = String::from_utf8_lossy(&text.stdout);
    println!("openssl x509 -text:\n{tout}");
    assert!(text.status.success(), "openssl x509 -text failed");
    assert!(
        tout.contains("DNS:example.com"),
        "SAN example.com missing:\n{tout}"
    );
    assert!(
        tout.contains("DNS:www.example.com"),
        "SAN www.example.com missing:\n{tout}"
    );
    assert!(
        tout.contains("sha256WithRSAEncryption"),
        "expected sha256WithRSAEncryption:\n{tout}"
    );
    assert!(
        tout.contains("TLS Web Server Authentication"),
        "expected serverAuth EKU:\n{tout}"
    );
}

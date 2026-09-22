//! URL components belong to URL.prototype, not to an instance's own keys.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn url_components_are_prototype_accessors_and_expandos_stay_separate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
const u: any = new URL("https://e.com/p?q=1");
const proto: any = Object.getPrototypeOf(u);
console.log("PROTO", proto === URL.prototype);
console.log("OWN", Object.keys(u).length, Object.getOwnPropertyNames(u).length);
const href = Object.getOwnPropertyDescriptor(proto, "href")!;
const origin = Object.getOwnPropertyDescriptor(proto, "origin")!;
console.log("DESCRIPTOR", typeof href.get, typeof href.set, href.enumerable, href.configurable,
    typeof origin.get, origin.set === undefined);
console.log("READ", u.href, u.searchParams.get("q"));
console.log("BORROW", href.get!.call(u));
u.pathname = "/next";
u.search = "?x=2";
console.log("WRITE", u.href, u.searchParams.get("x"));
u.extra = 42;
console.log("EXPANDO", Object.keys(u).join(","), Object.getOwnPropertyNames(u).join(","), u.extra, u.href);
u.host = "other.com:8080";
console.log("HOST", u.host, u.hostname, u.port, u.origin);
u.host = "third.com";
console.log("PORT_PRESERVE", u.host, u.port);
const d: any = new URL("https://defined.com/a");
Object.defineProperty(d, "extra", {value: 7, enumerable: true, configurable: true});
console.log("DEFINE", d.href, Object.keys(d).join(","), d.extra);
"#,
    )
    .expect("write entry");
    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "run failed, stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(stdout.contains("PROTO true"), "{stdout}");
    assert!(stdout.contains("OWN 0 0"), "{stdout}");
    assert!(
        stdout.contains("DESCRIPTOR function function true true function true"),
        "{stdout}"
    );
    assert!(stdout.contains("READ https://e.com/p?q=1 1"), "{stdout}");
    assert!(stdout.contains("BORROW https://e.com/p?q=1"), "{stdout}");
    assert!(
        stdout.contains("WRITE https://e.com/next?x=2 2"),
        "{stdout}"
    );
    assert!(
        stdout.contains("EXPANDO extra extra 42 https://e.com/next?x=2"),
        "{stdout}"
    );
    assert!(
        stdout.contains("HOST other.com:8080 other.com 8080 https://other.com:8080"),
        "{stdout}"
    );
    assert!(
        stdout.contains("PORT_PRESERVE third.com:8080 8080"),
        "{stdout}"
    );
    assert!(
        stdout.contains("DEFINE https://defined.com/a extra 7"),
        "{stdout}"
    );
}

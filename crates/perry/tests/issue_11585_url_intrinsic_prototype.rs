//! #11585: a module-level `function URL` shadows the global binding. The
//! native URL constructor, still reachable as `new globalThis.URL(...)`, must
//! link its instances to %URL.prototype% — not to whatever the global `URL`
//! binding's `.prototype` currently is — or every WebIDL component accessor
//! (`hostname`, `href`, ...) reads `undefined` (test_issue_5912, since
//! a9c6b0202 moved the components onto URL.prototype).

mod support;

const SOURCE: &str = r#"
function URL(url?: string) {
  return { url: url ?? "default", kind: "local" };
}
console.log(JSON.stringify(new URL()));
const real = new (globalThis as any).URL("https://user:pw@example.com:8080/p/a?q=1#h");
console.log(real.hostname, real.port, real.pathname, real.search, real.hash);
console.log(real.href);
console.log(String(real) === real.href, typeof real.searchParams.get);
"#;

#[test]
fn native_url_instances_keep_the_intrinsic_prototype_when_url_is_shadowed() {
    assert_eq!(
        support::compile_and_run(SOURCE),
        concat!(
            "{\"url\":\"default\",\"kind\":\"local\"}\n",
            "example.com 8080 /p/a ?q=1 #h\n",
            "https://user:pw@example.com:8080/p/a?q=1#h\n",
            "true function\n",
        )
    );
}

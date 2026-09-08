use super::*;

#[test]
fn fastify_examples_request_specialized_compilation_without_being_skipped() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples");
    for file in [
        "getting-started/npm_packages.ts",
        "stdlib/http/fastify_json.ts",
        "stdlib/overview/snippets.ts",
    ] {
        let banner = read_banner(&root.join(file)).unwrap();
        assert!(
            banner.requires_auto_optimize,
            "{file}: must rebuild the Fastify pump"
        );
        assert!(
            banner.compile_only,
            "{file}: requires external services to run"
        );
        for host in ["macos", "linux", "windows"] {
            assert!(
                banner.platforms.contains(host),
                "{file}: {host} must compile it"
            );
        }
    }
}

#[test]
fn unknown_requirement_is_an_error_instead_of_silently_disabling_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("typo.ts");
    std::fs::write(
        &path,
        "// requires: auto-optmize\nconsole.log('example');\n",
    )
    .unwrap();
    let error = read_banner(&path).unwrap_err().to_string();
    assert!(error.contains("typo.ts"));
    assert!(error.contains("unknown doc-example requirement `auto-optmize`"));
}

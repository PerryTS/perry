use std::collections::BTreeSet;

#[test]
fn agent_tls_bridge_covers_the_frame_abi_and_restores_nested_sdk_contexts() {
    let frame = include_str!("../guest_frame.h");
    let agent = include_str!("../guest_agent.h");
    let declarations: BTreeSet<_> = frame
        .lines()
        .filter_map(|line| {
            let (signature, _) = line.split_once('(')?;
            let name = signature.split_whitespace().last()?.trim_start_matches('*');
            (name.starts_with("perry_component_frame_")
                || name.starts_with("js_component_set_guest_"))
            .then_some(name)
        })
        .collect();
    let bridged: BTreeSet<_> = agent
        .lines()
        .filter_map(|line| line.strip_prefix("#define "))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| {
            name.starts_with("perry_component_frame_")
                || name.starts_with("js_component_set_guest_")
        })
        .collect();
    assert_eq!(
        declarations, bridged,
        "new frame calls must select agent TLS"
    );
    assert!(declarations.len() >= 52);

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("perry_component_frame.h"), frame).unwrap();
    std::fs::write(dir.path().join("perry_component_agent.h"), agent).unwrap();
    let source = dir.path().join("agent.c");
    std::fs::write(&source, include_str!("tests/agent_tls.c")).unwrap();
    let exe = dir.path().join("agent");
    let output = std::process::Command::new("clang")
        .args([
            "-std=c11",
            "-Wall",
            "-Werror",
            "-Wno-unused-function",
            "-O2",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = std::process::Command::new(exe).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        "PASS: agent TLS, nested calls, errors, void and wide results"
    );
}

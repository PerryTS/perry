//! Cross-backend conformance suite.
//!
//! These tests run the same questions against every CliProtocol
//! implementation. Their job is to make "do all backends behave the
//! same way?" a CI-blocking unit test, not a runtime surprise.
//!
//! Three categories:
//!
//! - **Universals**: features every backend MUST support (image arg,
//!   `--name`, `-p`, `-v`, `-e`, `--label`, etc.). A protocol that
//!   silently drops one of these is broken.
//!
//! - **Capability-gated**: features that are documented as
//!   per-backend on `BackendCapabilities` (privileged, seccomp, etc.).
//!   Each protocol's behavior is checked against its own capability
//!   declaration — declared `Native` MUST emit the flag; declared
//!   `Unsupported` MUST drop it (the normalization layer in
//!   `CliBackend::run_with_security` handles this, so the protocol
//!   itself never sees those fields after normalization — but if a
//!   user calls `run_args` directly, the protocol must still produce
//!   ARGS that are valid for its CLI).
//!
//! - **Output normalization**: parse_list_output / parse_inspect_output
//!   on every protocol must produce a `ContainerInfo` with the same
//!   field semantics regardless of which backend emitted the JSON.

use perry_container_compose::backend::{
    AppleContainerProtocol, CliProtocol, DockerProtocol, LimaProtocol,
};
use perry_container_compose::capabilities::{
    normalise_spec_for, BackendCapabilities, FeatureSupport,
};
use perry_container_compose::types::ContainerSpec;

/// All four protocols, paired with their identifying capability.
fn all_protocols() -> Vec<(&'static str, Box<dyn CliProtocol>)> {
    vec![
        ("docker", Box::new(DockerProtocol)),
        // Podman uses the DockerProtocol shape (the CLIs are wire-compatible
        // for the subset Perry uses). Including it explicitly so adding a
        // dedicated `PodmanProtocol` later can be a drop-in.
        ("podman", Box::new(DockerProtocol)),
        ("apple", Box::new(AppleContainerProtocol)),
        (
            "lima",
            Box::new(LimaProtocol {
                instance: "default".into(),
            }),
        ),
    ]
}

fn baseline_spec() -> ContainerSpec {
    ContainerSpec {
        image: "nginx:alpine".into(),
        name: Some("web".into()),
        ports: Some(vec!["8080:80".into()]),
        volumes: Some(vec!["data:/var/www".into()]),
        env: Some([("LOG_LEVEL".into(), "debug".into())].into()),
        labels: Some([("perry.compose.project".into(), "demo".into())].into()),
        ..Default::default()
    }
}

#[test]
fn universal_run_emits_image() {
    // Every backend MUST emit the image name. This is the canary —
    // a protocol that drops the image is fundamentally broken.
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.iter().any(|a| a == &spec.image),
            "{name}: run_args must include image; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_name() {
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2).any(|w| w[0] == "--name" && w[1] == "web"),
            "{name}: run_args must emit --name web; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_ports() {
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2).any(|w| w[0] == "-p" && w[1] == "8080:80"),
            "{name}: run_args must emit -p 8080:80; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_volumes() {
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-v" && w[1] == "data:/var/www"),
            "{name}: run_args must emit -v data:/var/www; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_env() {
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-e" && w[1] == "LOG_LEVEL=debug"),
            "{name}: run_args must emit -e LOG_LEVEL=debug; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_labels() {
    // Project labels are how `downByProject` finds resources later.
    // Every backend MUST emit them or the cleanup API breaks.
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--label" && w[1] == "perry.compose.project=demo"),
            "{name}: run_args must emit project label; got {:?}",
            args
        );
    }
}

#[test]
fn universal_run_emits_network_alias() {
    // Protocol-layer contract: when given a `network_aliases` field,
    // every backend's `run_args()` emits the corresponding flag. The
    // protocol is a "dumb emitter" — it doesn't decide whether the
    // field is appropriate for the backend; that's the engine's job
    // via `normalise_spec_for(caps, spec)` BEFORE `run_args()` is
    // called. So this test directly exercises the protocol with the
    // field set; on apple/container, the engine pipeline strips
    // network_aliases pre-emit (per
    // BackendCapabilities::APPLE.network_alias = Unsupported) — the
    // protocol layer never sees it in production.
    //
    // Note (post-v0.5.380 audit): apple/container 0.12 does NOT
    // actually accept `--network-alias` (verified via `container run
    // --help` — only `--network`). The capability table was corrected
    // to mark it Unsupported, and the engine drops the field. This
    // protocol-layer test still passes because the protocol itself
    // emits whatever it's given, but the production code path never
    // emits the flag against apple.
    for (name, proto) in all_protocols() {
        let spec = ContainerSpec {
            image: "alpine".into(),
            network: Some("appnet".into()),
            network_aliases: Some(vec!["db".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "db"),
            "{name}: run_args must emit --network-alias db; got {:?}",
            args
        );
    }
}

#[test]
fn universal_remove_args_emit_force_flag() {
    for (name, proto) in all_protocols() {
        let args = proto.remove_args("c123", true);
        // Either `-f` (Docker short form) or `--force` (apple) is fine —
        // they're equivalent. Just need SOMETHING that says force.
        assert!(
            args.iter().any(|a| a == "-f" || a == "--force"),
            "{name}: remove_args(force=true) must emit -f or --force; got {:?}",
            args
        );
    }
}

#[test]
fn universal_logs_args_emit_tail_count() {
    // Every backend exposes some way to limit log lines. The flag
    // differs (`--tail` for docker, `-n` for apple) but the value is
    // somewhere in the args.
    for (name, proto) in all_protocols() {
        let args = proto.logs_args("c123", Some(42));
        assert!(
            args.iter().any(|a| a == "42"),
            "{name}: logs_args(tail=42) must emit `42` somewhere; got {:?}",
            args
        );
    }
}

#[test]
fn universal_inspect_args_target_id() {
    for (name, proto) in all_protocols() {
        let args = proto.inspect_args("c123");
        assert!(
            args.last().map(|s| s == "c123").unwrap_or(false),
            "{name}: inspect_args last arg must be the id; got {:?}",
            args
        );
    }
}

#[test]
fn universal_pull_args_target_reference() {
    for (name, proto) in all_protocols() {
        let args = proto.pull_image_args("alpine:3.20");
        assert!(
            args.last().map(|s| s == "alpine:3.20").unwrap_or(false),
            "{name}: pull_image_args last arg must be the reference; got {:?}",
            args
        );
    }
}

// ---------- Capability-gated divergence ----------

#[test]
fn capability_apple_drops_privileged_via_normalization() {
    // The contract: `BackendCapabilities::APPLE` declares `privileged:
    // Unsupported`. Running the normaliser must drop the field.
    let mut spec = ContainerSpec {
        image: "alpine".into(),
        privileged: Some(true),
        ..Default::default()
    };
    let warnings = normalise_spec_for(&BackendCapabilities::APPLE, "svc", &mut spec);
    assert_eq!(spec.privileged, None);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn capability_docker_keeps_privileged() {
    let mut spec = ContainerSpec {
        image: "alpine".into(),
        privileged: Some(true),
        ..Default::default()
    };
    let warnings = normalise_spec_for(&BackendCapabilities::DOCKER, "svc", &mut spec);
    assert_eq!(spec.privileged, Some(true));
    assert!(warnings.is_empty());
}

#[test]
fn capabilities_consistent_per_protocol() {
    // Every protocol's `capabilities()` must point at the matching
    // `BackendCapabilities::*` constant.
    let docker = DockerProtocol;
    assert_eq!(docker.capabilities().backend, "docker");

    let apple = AppleContainerProtocol;
    assert_eq!(apple.capabilities().backend, "apple");

    let lima = LimaProtocol {
        instance: "default".into(),
    };
    assert_eq!(lima.capabilities().backend, "lima");
}

#[test]
fn apple_unsupported_set_documented() {
    // Pin the exact set of features apple/container 0.12 doesn't
    // support. If a future apple release adds support for one,
    // flip the field on `BackendCapabilities::APPLE` and update
    // this test — the orchestrator + normaliser pick it up
    // automatically.
    let caps = &BackendCapabilities::APPLE;
    assert!(matches!(caps.privileged, FeatureSupport::Unsupported));
    assert!(matches!(caps.seccomp_profile, FeatureSupport::Unsupported));
    assert!(matches!(
        caps.no_new_privileges,
        FeatureSupport::Unsupported
    ));
    assert!(matches!(caps.internal_network, FeatureSupport::Unsupported));
    assert!(matches!(
        caps.ipc_namespace_share,
        FeatureSupport::Unsupported
    ));
    assert!(matches!(
        caps.pid_namespace_share,
        FeatureSupport::Unsupported
    ));
}

#[test]
fn apple_emulated_features_documented() {
    let caps = &BackendCapabilities::APPLE;
    assert!(matches!(caps.restart_policy, FeatureSupport::Emulated));
    assert!(matches!(caps.healthcheck_native, FeatureSupport::Emulated));
    assert!(matches!(
        caps.image_signature_verify,
        FeatureSupport::Emulated
    ));
}

// ---------- Output normalization ----------

#[test]
fn parse_list_output_returns_unified_container_info_shape() {
    // Each backend's parser should yield ContainerInfo with the same
    // field semantics. We check that a populated entry yields an
    // info with non-empty id, image, and status — regardless of
    // backend.

    // Docker shape (NDJSON line)
    let docker_stdout = r#"{"ID":"abc","Names":["web"],"Image":"nginx","Status":"Up 5 seconds","Created":"2026-04-28T00:00:00Z","Ports":[],"Labels":{}}"#;
    let docker_infos = DockerProtocol.parse_list_output(docker_stdout).unwrap();
    assert_eq!(docker_infos.len(), 1);
    assert_eq!(docker_infos[0].id, "abc");
    assert_eq!(docker_infos[0].image, "nginx");

    // Apple shape (JSON array)
    let apple_stdout = r#"[{"configuration":{"id":"abc","image":{"reference":"nginx"},"hostname":"web","labels":{}},"status":"running","networks":[]}]"#;
    let apple_infos = AppleContainerProtocol
        .parse_list_output(apple_stdout)
        .unwrap();
    assert_eq!(apple_infos.len(), 1);
    assert_eq!(apple_infos[0].id, "abc");
    assert_eq!(apple_infos[0].image, "nginx");

    // The orchestrator can read `info.id` and `info.image` from either
    // backend without a per-backend branch — this is the "deterministic
    // behavior" guarantee in the cleanup-by-project / drift-detection paths.
}

#[test]
fn parse_inspect_output_returns_unified_shape() {
    // Same canary at the inspect layer.
    let docker_stdout = r#"[{"Id":"abc","Name":"/web","Config":{"Image":"nginx","Labels":{}},"State":{"Status":"running"},"Created":"2026-04-28T00:00:00Z","NetworkSettings":{"IPAddress":"172.17.0.2","Networks":{}}}]"#;
    let docker_info = DockerProtocol.parse_inspect_output(docker_stdout).unwrap();
    assert_eq!(docker_info.id, "abc");
    assert_eq!(docker_info.status, "running");

    let apple_stdout = r#"[{"configuration":{"id":"abc","image":{"reference":"nginx"},"hostname":"web","labels":{}},"status":"running","networks":[{"address":"10.0.0.5"}]}]"#;
    let apple_info = AppleContainerProtocol
        .parse_inspect_output(apple_stdout)
        .unwrap();
    assert_eq!(apple_info.id, "abc");
    assert_eq!(apple_info.status, "running");
    assert_eq!(apple_info.ip_address, "10.0.0.5");
}

#[test]
fn parse_container_id_strips_whitespace_uniformly() {
    // `run --detach` returns the ID with trailing newline on every
    // backend. Parsers must normalise.
    for (name, proto) in all_protocols() {
        let id = proto.parse_container_id("abc123\n").unwrap();
        assert_eq!(
            id, "abc123",
            "{name}: parse_container_id must strip newline"
        );
        let id2 = proto.parse_container_id("  abc123  \n").unwrap();
        assert_eq!(id2, "abc123", "{name}: parse_container_id must trim");
    }
}

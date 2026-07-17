use crate::error::Result;
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod apple;
mod cli_backend;
mod detect;
mod docker;
mod lima;

#[cfg(test)]
pub(crate) use apple::split_image_reference;
pub use apple::AppleContainerProtocol;
pub use cli_backend::CliBackend;
pub use detect::{detect_backend, platform_candidates, probe_all_candidates};
pub use docker::DockerProtocol;
pub use lima::LimaProtocol;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendProbeResult {
    pub name: String,
    pub available: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SecurityProfile {
    pub read_only_root: bool,
    /// Path to a seccomp JSON profile, or the literal string `"default"`
    /// to use the runtime's default profile. Emitted as
    /// `--security-opt seccomp=<value>`. Maps to the user's
    /// `security_opt: ["seccomp=..."]` entries on `ComposeService`.
    pub seccomp: Option<String>,
    /// `--security-opt no-new-privileges`. SUID/SGID binaries inside
    /// the container can't gain privileges via execve. Maps to the
    /// user's `security_opt: ["no-new-privileges"]` (or `:true` /
    /// `=true`) entries.
    pub no_new_privileges: bool,
}

impl SecurityProfile {
    /// Parse a `security_opt: Vec<String>` from `ComposeService` into
    /// the structured `SecurityProfile`. Pre-fix the engine had a
    /// `// Could be parsed from security_opt` TODO and silently
    /// dropped these fields — a security regression where users
    /// thought they were hardening containers but the flags never
    /// reached the runtime.
    ///
    /// Recognised entries (compose-spec §service.security_opt):
    /// - `"seccomp=<path>"` / `"seccomp:<path>"` → `seccomp`
    /// - `"seccomp=default"` → `seccomp = Some("default")`
    /// - `"no-new-privileges"` / `"no-new-privileges:true"` /
    ///   `"no-new-privileges=true"` → `no_new_privileges = true`
    ///
    /// Unrecognised entries are ignored (left for the caller's
    /// future support; `tracing::warn!` could be added if desired).
    pub fn merge_security_opt(&mut self, security_opt: &[String]) {
        for opt in security_opt {
            // seccomp=<path> or seccomp:<path>
            if let Some(rest) = opt
                .strip_prefix("seccomp=")
                .or_else(|| opt.strip_prefix("seccomp:"))
            {
                self.seccomp = Some(rest.to_string());
                continue;
            }
            // no-new-privileges, no-new-privileges:true, no-new-privileges=true
            if opt == "no-new-privileges"
                || opt == "no-new-privileges:true"
                || opt == "no-new-privileges=true"
            {
                self.no_new_privileges = true;
                continue;
            }
        }
    }
}

#[async_trait]
pub trait ContainerBackend: Send + Sync {
    fn backend_name(&self) -> &str;

    /// What this backend can do. The engine reads this to decide which
    /// `ContainerSpec` fields to drop / translate / hard-reject before
    /// calling `run_with_security`. Default returns the Docker baseline
    /// (everything supported); concrete backends should override.
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::DOCKER
    }

    async fn check_available(&self) -> Result<()>;
    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()>;
    async fn remove(&self, id: &str, force: bool) -> Result<()>;
    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>>;
    async fn inspect(&self, id: &str) -> Result<ContainerInfo>;
    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs>;
    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs>;
    async fn pull_image(&self, reference: &str) -> Result<()>;
    async fn list_images(&self) -> Result<Vec<ImageInfo>>;
    async fn remove_image(&self, reference: &str, force: bool) -> Result<()>;
    async fn create_network(&self, name: &str, config: &ComposeNetwork) -> Result<()>;
    async fn remove_network(&self, name: &str) -> Result<()>;
    async fn create_volume(&self, name: &str, config: &ComposeVolume) -> Result<()>;
    async fn remove_volume(&self, name: &str) -> Result<()>;
    async fn inspect_network(&self, name: &str) -> Result<()>;
    async fn inspect_volume(&self, name: &str) -> Result<()>;
    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo>;
    async fn build(&self, spec: &ComposeServiceBuild, image_name: &str) -> Result<()>;
    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle>;
    /// Security-aware variant of [`create`](Self::create): same
    /// contract as [`run_with_security`](Self::run_with_security)
    /// (capability normalization + `security_args` splicing) but the
    /// container is created without being started. Exists so the
    /// public `create()` API can honor spec-level `seccomp` /
    /// `no_new_privileges` instead of silently dropping them.
    async fn create_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle>;
    /// Wait for a container to exit and return its exit code.
    async fn wait(&self, id: &str) -> Result<i32>;
}

pub trait CliProtocol: Send + Sync {
    fn subcommand_prefix(&self) -> Option<&str> {
        None
    }

    /// What this backend can do. Drives the spec-normalization pass that
    /// keeps cross-backend behavior deterministic — see
    /// `crate::capabilities` for the architecture writeup.
    ///
    /// Default impl returns `BackendCapabilities::DOCKER` (the
    /// "everything supported" baseline) — protocols that diverge from
    /// the Docker reference override this.
    fn capabilities(&self) -> &'static crate::capabilities::BackendCapabilities {
        &crate::capabilities::BackendCapabilities::DOCKER
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn start_args(&self, id: &str) -> Vec<String>;
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String>;
    fn remove_args(&self, id: &str, force: bool) -> Vec<String>;
    fn list_args(&self, all: bool) -> Vec<String>;
    fn inspect_args(&self, id: &str) -> Vec<String>;
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String>;
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String>;
    fn pull_image_args(&self, reference: &str) -> Vec<String>;
    fn list_images_args(&self) -> Vec<String>;
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String>;
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String>;
    fn remove_network_args(&self, name: &str) -> Vec<String>;
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String>;
    fn remove_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_network_args(&self, name: &str) -> Vec<String>;
    fn inspect_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_image_args(&self, reference: &str) -> Vec<String>;
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String>;
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String>;

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>>;
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo>;
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>>;
    fn parse_container_id(&self, stdout: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ComposeError;
    use crate::types::ContainerSpec;

    #[test]
    fn test_docker_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            name: Some("web".into()),
            ports: Some(vec!["80:80".into()]),
            env: Some([("FOO".into(), "BAR".into())].into()),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"web".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"80:80".to_string()));
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"FOO=BAR".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"nginx".to_string()));
    }

    #[test]
    fn test_docker_run_args_includes_network_alias() {
        // Service-key network alias regression: pre-fix Perry's compose
        // engine relied on `container_name` for cross-service DNS,
        // breaking any port of a docker-compose stack from the wider
        // ecosystem. The fix populates `network_aliases` from the
        // service KEY in `ComposeEngine::up`; this test pins that
        // `--network-alias <name>` is emitted per entry.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "postgres:16-alpine".into(),
            name: Some("myapp_db_abc12345".into()),
            network: Some("myapp_appnet".into()),
            network_aliases: Some(vec!["db".into(), "primary-db".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "db"),
            "expected --network-alias db; got {:?}",
            args
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "primary-db"),
            "expected --network-alias primary-db; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_emits_seccomp_when_set() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            // seccomp is spec-level since the run()/create() security
            // fix, but it still reaches the CLI via security_args
            // (spliced in by run_with_security/create_with_security),
            // NOT via run_args. Test the security_args output directly:
            ..Default::default()
        };
        let _ = proto.run_args(&spec); // smoke — no panic on minimal spec
        let security_args = proto.security_args(&SecurityProfile {
            read_only_root: true,
            seccomp: Some("default".into()),
            ..Default::default()
        });
        assert!(
            security_args.iter().any(|s| s.contains("seccomp")),
            "expected seccomp in security args; got {:?}",
            security_args
        );
    }

    #[test]
    fn test_container_spec_json_parses_security_fields() {
        // Pins the FFI boundary: the TS object literal passed to
        // `run()`/`create()` arrives as JSON and is serde-parsed into
        // `ContainerSpec`. Pre-fix, `seccomp` / `no_new_privileges`
        // were not fields on the struct, so serde silently dropped
        // them and the documented hardening never reached the runtime.
        let spec: ContainerSpec = serde_json::from_str(
            r#"{
                "image": "alpine:3.19",
                "read_only": true,
                "user": "nobody",
                "cap_drop": ["ALL"],
                "seccomp": "default",
                "no_new_privileges": true
            }"#,
        )
        .expect("security fields must parse");
        assert_eq!(spec.seccomp.as_deref(), Some("default"));
        assert_eq!(spec.no_new_privileges, Some(true));
        assert!(spec.has_security_opts());

        let profile = spec.security_profile();
        assert!(profile.read_only_root);
        assert_eq!(profile.seccomp.as_deref(), Some("default"));
        assert!(profile.no_new_privileges);
    }

    #[test]
    fn test_container_spec_without_security_fields_uses_plain_path() {
        // `read_only` / `cap_drop` / `user` are emitted directly by
        // run_args/create_args, so a spec using only those must NOT
        // trigger the security-aware path (routing invariant for
        // `js_container_run` / `js_container_create`).
        let spec: ContainerSpec = serde_json::from_str(
            r#"{ "image": "alpine", "read_only": true, "cap_drop": ["ALL"], "user": "nobody" }"#,
        )
        .expect("parse");
        assert!(!spec.has_security_opts());
    }

    #[test]
    fn test_docker_run_with_security_argv_from_spec() {
        // End-to-end argv shape for the fixed `run()` path: spec-level
        // security fields → SecurityProfile → security_args spliced
        // before the image reference. Mirrors what
        // `CliBackend::run_with_security` executes (minus the process
        // spawn).
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine:3.19".into(),
            cmd: Some(vec!["echo".into(), "hi".into()]),
            read_only: Some(true),
            user: Some("nobody".into()),
            cap_drop: Some(vec!["ALL".into()]),
            seccomp: Some("default".into()),
            no_new_privileges: Some(true),
            ..Default::default()
        };
        let args = cli_backend::splice_security_args(
            proto.run_args(&spec),
            &spec.image,
            proto.security_args(&spec.security_profile()),
        );

        assert!(
            args.windows(2)
                .any(|w| w[0] == "--security-opt" && w[1] == "seccomp=default"),
            "expected --security-opt seccomp=default; got {:?}",
            args
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--security-opt" && w[1] == "no-new-privileges:true"),
            "expected --security-opt no-new-privileges:true; got {:?}",
            args
        );
        assert!(args.contains(&"--read-only".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--user" && w[1] == "nobody"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--cap-drop" && w[1] == "ALL"));

        // Every flag must precede the image; the container command
        // (`echo hi`) must stay after it.
        let image_pos = args.iter().position(|a| a == "alpine:3.19").unwrap();
        let seccomp_pos = args.iter().position(|a| a == "seccomp=default").unwrap();
        let echo_pos = args.iter().position(|a| a == "echo").unwrap();
        assert!(
            seccomp_pos < image_pos && image_pos < echo_pos,
            "flag/image/cmd ordering broken: {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_with_security_argv_when_option_value_equals_image() {
        // Regression: the image slot used to be located by string value,
        // so a container name equal to the image reference matched the
        // `--name` VALUE first and the security flags were spliced
        // between `--name` and its value:
        //   run --name --security-opt seccomp=default alpine alpine
        // which corrupts both the name and the command. The image is now
        // located by construction, so duplicate values are harmless.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            name: Some("alpine".into()),
            cmd: Some(vec!["alpine".into()]),
            seccomp: Some("default".into()),
            ..Default::default()
        };
        let args = cli_backend::build_secured_args(
            &proto,
            &spec,
            &spec.security_profile(),
            /* create_only */ false,
        );

        // `--name` keeps its value, and no flag separates the two.
        let name_pos = args.iter().position(|a| a == "--name").unwrap();
        assert_eq!(
            args[name_pos + 1],
            "alpine",
            "--name lost its value: {:?}",
            args
        );

        // Ordering: security flag → image → command, with exactly three
        // `alpine` tokens (name value, image, command).
        let seccomp_pos = args.iter().position(|a| a == "seccomp=default").unwrap();
        assert!(
            seccomp_pos > name_pos + 1,
            "security flags spliced into the --name pair: {:?}",
            args
        );
        assert_eq!(
            args.iter().filter(|a| *a == "alpine").count(),
            3,
            "expected name value + image + cmd: {:?}",
            args
        );
        // The image is the token right after the last security flag.
        assert_eq!(
            args[seccomp_pos + 1],
            "alpine",
            "image must directly follow the security flags: {:?}",
            args
        );
        // No sentinel may leak into the executed argv.
        assert!(
            !args.iter().any(|a| a.contains('\u{1}')),
            "image sentinel leaked: {:?}",
            args
        );
    }

    #[test]
    fn test_docker_create_with_security_argv_from_spec() {
        // Same shape for the fixed `create()` path — `create` has no
        // security-arg splice pre-fix (there was no
        // `create_with_security` at all), so pin the argv here.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            seccomp: Some("/etc/seccomp/app.json".into()),
            ..Default::default()
        };
        let args = cli_backend::splice_security_args(
            proto.create_args(&spec),
            &spec.image,
            proto.security_args(&spec.security_profile()),
        );
        assert_eq!(args[0], "create");
        let sec_pos = args
            .iter()
            .position(|a| a == "seccomp=/etc/seccomp/app.json")
            .expect("expected seccomp security-opt in create argv");
        let image_pos = args.iter().position(|a| a == "nginx").unwrap();
        assert!(
            sec_pos < image_pos,
            "security args must precede the image; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_security_argv_from_spec_drops_seccomp_keeps_read_only() {
        // apple/container has no seccomp equivalent: the normalization
        // layer drops the field with a warning AND the protocol's
        // security_args ignores it (defense in depth). A spec-driven
        // launch on apple must still emit `--read-only` but never a
        // seccomp flag.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            read_only: Some(true),
            seccomp: Some("default".into()),
            ..Default::default()
        };
        let args = cli_backend::splice_security_args(
            proto.run_args(&spec),
            &spec.image,
            proto.security_args(&spec.security_profile()),
        );
        assert!(args.contains(&"--read-only".to_string()));
        assert!(
            !args.iter().any(|s| s.contains("seccomp")),
            "apple argv must not contain seccomp; got {:?}",
            args
        );
    }

    #[test]
    fn test_compose_service_security_opt_flows_into_container_spec() {
        // `ComposeService::to_container_spec` must parse
        // `security_opt: [...]` into the spec-level fields so
        // `run_command` routes through the security-aware path instead
        // of dropping the entries on the plain-`run` floor.
        let svc = crate::types::ComposeService {
            image: Some("redis:7".into()),
            security_opt: Some(vec!["seccomp=default".into(), "no-new-privileges".into()]),
            ..Default::default()
        };
        let spec = svc.to_container_spec("cache", "cache-ctr");
        assert_eq!(spec.seccomp.as_deref(), Some("default"));
        assert_eq!(spec.no_new_privileges, Some(true));
        assert!(spec.has_security_opts());
    }

    #[test]
    fn test_docker_run_args_emits_entrypoint_array_form() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            entrypoint: Some(vec!["/usr/bin/env".into(), "sh".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let ep_idx = args
            .iter()
            .position(|s| s == "--entrypoint")
            .expect("expected --entrypoint flag");
        assert!(
            ep_idx + 1 < args.len(),
            "--entrypoint must have a value after it; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_omits_rm_when_unset() {
        // Conservative-default invariant: `rm: None` MUST NOT emit
        // `--rm`. Otherwise containers would silently auto-remove on
        // exit, defeating debug-after-failure workflows.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            rm: None,
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            !args.iter().any(|s| s == "--rm"),
            "rm: None must NOT emit --rm; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_run_args_omits_optional_flags_when_unset() {
        // Snapshot-style invariant: a minimal spec produces only
        // `run --detach <image>` plus image. No spurious flags.
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let unwanted = [
            "--privileged",
            "--read-only",
            "--user",
            "--workdir",
            "--cap-add",
            "--cap-drop",
            "--rm",
            "--name",
            "--network",
        ];
        for flag in unwanted {
            assert!(
                !args.iter().any(|s| s == flag),
                "minimal spec must NOT emit `{flag}`; got {:?}",
                args
            );
        }
    }

    #[test]
    fn test_apple_run_args_emits_detach_for_orchestrator() {
        // apple/container `run` is foreground-by-default. The orchestrator
        // needs the container ID back so it can move on — so `--detach`
        // is required, NOT prohibited. Pre-v0.5.374 the engine called the
        // foreground form and blocked on the container's main process,
        // making compose stacks effectively unworkable on apple/container.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.iter().any(|s| s == "--detach"),
            "apple/container run MUST include --detach for orchestrator; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_run_args_includes_network_alias() {
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            network: Some("appnet".into()),
            network_aliases: Some(vec!["worker".into()]),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--network-alias" && w[1] == "worker"),
            "apple/container should emit --network-alias too; got {:?}",
            args
        );
    }

    #[test]
    fn test_docker_security_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            privileged: Some(true),
            user: Some("nobody".into()),
            workdir: Some("/tmp".into()),
            cap_add: Some(vec!["NET_ADMIN".into()]),
            cap_drop: Some(vec!["ALL".into()]),
            read_only: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"--privileged".to_string()));
        assert!(args.contains(&"--user".to_string()));
        assert!(args.contains(&"nobody".to_string()));
        assert!(args.contains(&"--workdir".to_string()));
        assert!(args.contains(&"/tmp".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"NET_ADMIN".to_string()));
        assert!(args.contains(&"--cap-drop".to_string()));
        assert!(args.contains(&"ALL".to_string()));
        assert!(args.contains(&"--read-only".to_string()));
    }

    #[test]
    fn test_apple_run_args() {
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--detach".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"alpine".to_string()));
    }

    #[test]
    fn test_apple_run_args_drops_privileged() {
        // apple/container does NOT support `--privileged` (Linux
        // containers run inside an Apple-VM; host-privilege escalation
        // isn't a concept). We must silently drop it from the spec
        // rather than emit a flag the CLI rejects.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            privileged: Some(true),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        assert!(
            !args.iter().any(|s| s == "--privileged"),
            "apple/container must NOT emit --privileged; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_security_args_drops_seccomp() {
        // apple/container has no equivalent of Docker's
        // `--security-opt seccomp=<file>` (the syscall-filter model is
        // VM-host-managed). Honor only `--read-only`; drop seccomp.
        let proto = AppleContainerProtocol;
        let args = proto.security_args(&SecurityProfile {
            read_only_root: true,
            seccomp: Some("default".into()),
            ..Default::default()
        });
        assert!(args.iter().any(|s| s == "--read-only"));
        assert!(
            !args.iter().any(|s| s.contains("seccomp")),
            "apple/container security_args must drop seccomp; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_logs_uses_n_not_tail() {
        // apple/container's `logs` accepts `-n <N>` (the canonical name);
        // there is no `--tail` long form. Emitting `--tail` produces
        // "unknown flag" from the apple CLI.
        let proto = AppleContainerProtocol;
        let args = proto.logs_args("abc123", Some(50));
        assert_eq!(args[0], "logs");
        assert!(
            args.windows(2).any(|w| w[0] == "-n" && w[1] == "50"),
            "expected `-n 50`; got {:?}",
            args
        );
        assert!(
            !args.iter().any(|s| s == "--tail"),
            "apple/container must NOT emit --tail; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_list_uses_list_not_ps() {
        // apple/container has `list` / `ls` only — no `ps` alias.
        let proto = AppleContainerProtocol;
        let args = proto.list_args(true);
        assert_eq!(args[0], "list");
        assert!(args.contains(&"--format".to_string()));
        assert!(args.contains(&"json".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(
            !args.iter().any(|s| s == "ps"),
            "apple/container must NOT emit `ps`; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_inspect_drops_format_flag() {
        // apple/container's `inspect` outputs JSON natively. It does
        // NOT accept `--format` — emitting it produces "unknown flag".
        let proto = AppleContainerProtocol;
        let args = proto.inspect_args("abc123");
        assert_eq!(args[0], "inspect");
        assert!(
            !args.iter().any(|s| s == "--format"),
            "apple/container inspect must NOT emit --format; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_image_subcommand_routing() {
        // Image ops live under the `image` subcommand on apple/container.
        // Verify pull / list-images / remove-image / inspect-image all
        // route through it.
        let proto = AppleContainerProtocol;

        let pull = proto.pull_image_args("alpine:3.20");
        assert_eq!(&pull[..2], &["image".to_string(), "pull".to_string()]);
        assert_eq!(pull.last().unwrap(), "alpine:3.20");

        let list = proto.list_images_args();
        assert_eq!(&list[..2], &["image".to_string(), "list".to_string()]);
        assert!(list.iter().any(|s| s == "json"));

        let remove = proto.remove_image_args("alpine:3.20", true);
        assert_eq!(&remove[..2], &["image".to_string(), "delete".to_string()]);
        assert!(remove.iter().any(|s| s == "--force"));

        let inspect = proto.inspect_image_args("alpine:3.20");
        assert_eq!(&inspect[..2], &["image".to_string(), "inspect".to_string()]);
        // Inspect must NOT pass --format (apple outputs JSON natively)
        assert!(!inspect.iter().any(|s| s == "--format"));
    }

    #[test]
    fn test_apple_remove_uses_delete_canonical_form() {
        // apple/container's canonical removal is `delete` (with `rm` as
        // alias). Use the canonical name so logs read consistently.
        let proto = AppleContainerProtocol;
        let args = proto.remove_args("abc123", true);
        assert_eq!(args[0], "delete");
        assert!(args.iter().any(|s| s == "--force"));
    }

    #[test]
    fn test_apple_volume_create_drops_driver() {
        // apple/container's `volume create` does NOT accept `--driver`
        // (the volume model is local-only). The spec may carry a driver
        // string from a docker-compose file; we silently drop it.
        let proto = AppleContainerProtocol;
        let cfg = ComposeVolume {
            driver: Some("local".into()),
            ..Default::default()
        };
        let args = proto.create_volume_args("data", &cfg);
        assert_eq!(&args[..2], &["volume".to_string(), "create".to_string()]);
        assert!(
            !args.iter().any(|s| s == "--driver"),
            "apple/container volume create must NOT emit --driver; got {:?}",
            args
        );
        assert_eq!(args.last().unwrap(), "data");
    }

    #[test]
    fn test_apple_volume_remove_uses_delete() {
        let proto = AppleContainerProtocol;
        let args = proto.remove_volume_args("data");
        assert_eq!(args, vec!["volume", "delete", "data"]);
    }

    #[test]
    fn test_apple_network_create_drops_driver() {
        // apple/container's network model doesn't expose docker's
        // `--driver bridge` flag — the driver is implicit in the
        // apple-network plugin.
        let proto = AppleContainerProtocol;
        let cfg = ComposeNetwork {
            driver: Some("bridge".into()),
            ..Default::default()
        };
        let args = proto.create_network_args("appnet", &cfg);
        assert_eq!(&args[..2], &["network".to_string(), "create".to_string()]);
        assert!(
            !args.iter().any(|s| s == "--driver"),
            "apple/container network create must NOT emit --driver; got {:?}",
            args
        );
        assert_eq!(args.last().unwrap(), "appnet");
    }

    #[test]
    fn test_apple_network_remove_uses_delete() {
        let proto = AppleContainerProtocol;
        let args = proto.remove_network_args("appnet");
        assert_eq!(args, vec!["network", "delete", "appnet"]);
    }

    #[test]
    fn test_apple_create_args_no_detach() {
        // `create` has no detach concept — that's `start`'s job.
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            ..Default::default()
        };
        let args = proto.create_args(&spec);
        assert_eq!(args[0], "create");
        assert!(
            !args.iter().any(|s| s == "--detach"),
            "apple/container create must NOT emit --detach; got {:?}",
            args
        );
    }

    #[test]
    fn test_apple_parse_list_output_handles_empty_array() {
        let proto = AppleContainerProtocol;
        let infos = proto.parse_list_output("[]").expect("empty array parses");
        assert!(infos.is_empty());
    }

    #[test]
    fn test_apple_parse_list_output_apple_shape() {
        // Mirrors apple/container 0.12's `list --format json` shape:
        // a JSON array of `{ configuration: { id, image: { reference } },
        // status, networks: [{ address }] }` objects.
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "configuration": {
                    "id": "abc123def456",
                    "image": { "reference": "docker.io/library/alpine:3.20" },
                    "hostname": "alpine-test",
                    "labels": { "perry.compose.project": "test" }
                },
                "status": "running",
                "networks": [{ "address": "10.0.0.5" }]
            }
        ]"#;
        let infos = proto.parse_list_output(stdout).expect("parse ok");
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].id, "abc123def456");
        assert_eq!(infos[0].name, "alpine-test");
        assert_eq!(infos[0].image, "docker.io/library/alpine:3.20");
        assert_eq!(infos[0].status, "running");
        assert_eq!(infos[0].ip_address, "10.0.0.5");
        assert_eq!(
            infos[0].labels.get("perry.compose.project"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_apple_parse_inspect_output_apple_shape() {
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "configuration": {
                    "id": "ctr-id",
                    "image": { "reference": "alpine:latest" },
                    "hostname": "ctr-name",
                    "labels": {}
                },
                "status": "running",
                "networks": []
            }
        ]"#;
        let info = proto.parse_inspect_output(stdout).expect("parse ok");
        assert_eq!(info.id, "ctr-id");
        assert_eq!(info.name, "ctr-name");
        assert_eq!(info.image, "alpine:latest");
        assert_eq!(info.status, "running");
        assert_eq!(info.ip_address, "");
    }

    #[test]
    fn test_apple_parse_inspect_output_falls_back_to_docker_shape() {
        // Defensive: some apple-compatible runtimes emit docker-shaped
        // inspect output. The fallback parser should pick those up.
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "Id": "docker-id",
                "Name": "docker-name",
                "Config": { "Image": "alpine:latest", "Labels": {} },
                "State": { "Status": "running" },
                "Created": "2026-04-28T12:00:00Z",
                "NetworkSettings": { "IPAddress": "172.17.0.2", "Networks": {} }
            }
        ]"#;
        let info = proto.parse_inspect_output(stdout).expect("parse ok");
        assert_eq!(info.id, "docker-id");
        assert_eq!(info.name, "docker-name");
        assert_eq!(info.ip_address, "172.17.0.2");
    }

    #[test]
    fn test_apple_parse_list_images_output_apple_shape() {
        let proto = AppleContainerProtocol;
        let stdout = r#"[
            {
                "reference": "docker.io/library/alpine:3.20",
                "id": "sha256:abc123",
                "size": 7654321,
                "createdAt": "2026-04-01T00:00:00Z"
            },
            {
                "reference": "docker.io/library/postgres:16-alpine",
                "id": "sha256:def456",
                "size": 234567890
            }
        ]"#;
        let images = proto.parse_list_images_output(stdout).expect("parse ok");
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].repository, "docker.io/library/alpine");
        assert_eq!(images[0].tag, "3.20");
        assert_eq!(images[0].id, "sha256:abc123");
        assert_eq!(images[1].repository, "docker.io/library/postgres");
        assert_eq!(images[1].tag, "16-alpine");
    }

    #[test]
    fn test_split_image_reference_handles_registry_port() {
        // Registry hostname with port: `localhost:5000/repo:tag` must NOT
        // split on the registry's `:5000` colon.
        let (repo, tag) = split_image_reference("localhost:5000/repo:1.0");
        assert_eq!(repo, "localhost:5000/repo");
        assert_eq!(tag, "1.0");
    }

    #[test]
    fn test_split_image_reference_handles_digest() {
        let (repo, tag) = split_image_reference("alpine@sha256:abc123def456");
        assert_eq!(repo, "alpine");
        assert_eq!(tag, "sha256:abc123def456");
    }

    #[test]
    fn test_split_image_reference_defaults_to_latest() {
        let (repo, tag) = split_image_reference("alpine");
        assert_eq!(repo, "alpine");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_apple_run_args_includes_labels() {
        // The compose engine writes `perry.compose.project` and
        // `perry.compose.spec_hash` labels on every container; these
        // drive `downByProject` cleanup and spec-drift detection. Pin
        // that apple emits them.
        let proto = AppleContainerProtocol;
        let mut labels = HashMap::new();
        labels.insert("perry.compose.project".into(), "myproj".into());
        labels.insert("perry.compose.spec_hash".into(), "abcd1234".into());
        let spec = ContainerSpec {
            image: "alpine".into(),
            labels: Some(labels),
            ..Default::default()
        };
        let args = proto.run_args(&spec);
        let label_pairs: Vec<&str> = args
            .windows(2)
            .filter(|w| w[0] == "--label")
            .map(|w| w[1].as_str())
            .collect();
        assert!(
            label_pairs
                .iter()
                .any(|s| *s == "perry.compose.project=myproj"),
            "expected project label; got {:?}",
            label_pairs
        );
        assert!(
            label_pairs
                .iter()
                .any(|s| *s == "perry.compose.spec_hash=abcd1234"),
            "expected spec_hash label; got {:?}",
            label_pairs
        );
    }

    #[test]
    fn test_lima_run_args() {
        let proto = LimaProtocol {
            instance: "default".into(),
        };
        let spec = ContainerSpec {
            image: "busybox".into(),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert_eq!(args[0], "shell");
        assert_eq!(args[1], "default");
        assert_eq!(args[2], "nerdctl");
        assert_eq!(args[3], "run");
    }

    #[test]
    fn test_platform_candidates() {
        let candidates = platform_candidates();
        assert!(!candidates.is_empty());
        if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
            assert_eq!(candidates[0], "apple/container");
        } else {
            assert_eq!(candidates[0], "podman");
        }
    }

    /// All env-var-mutating tests in one function. cargo runs tests
    /// in parallel by default and `std::env::set_var` is process-global,
    /// so independent `#[tokio::test]` cases would race the env var
    /// across threads and produce flaky results. Consolidate sequentially
    /// rather than depend on a serial-test crate (avoids the dep + the
    /// per-test setup overhead of `#[serial]`).
    #[tokio::test]
    async fn test_detect_backend_env_override_behavior() {
        // -------------------------------------------------------------
        // Phase 1: single name (existing behavior, backwards-compat)
        // -------------------------------------------------------------
        std::env::set_var("PERRY_CONTAINER_BACKEND", "invalid-backend-name");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 1);
            assert_eq!(probed[0].name, "invalid-backend-name");
            assert_eq!(probed[0].reason, "unknown backend");
        } else {
            panic!("Expected NoBackendFound error from single-name override");
        }

        // -------------------------------------------------------------
        // Phase 2: comma-separated user priority list (v0.5.380 feature)
        // -------------------------------------------------------------
        // Each name in the list gets probed in order. All-invalid case:
        // returns NoBackendFound with one BackendProbeResult per
        // attempted name, order preserved.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "bogus-one,bogus-two,bogus-three");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 3, "expected one probe per name");
            assert_eq!(probed[0].name, "bogus-one");
            assert_eq!(probed[1].name, "bogus-two");
            assert_eq!(probed[2].name, "bogus-three");
            assert!(probed.iter().all(|p| p.reason.contains("unknown")));
        } else {
            panic!("Expected NoBackendFound error from comma-separated list");
        }

        // -------------------------------------------------------------
        // Phase 3: tolerant parsing — whitespace + empty entries
        // -------------------------------------------------------------
        // Real env-var input `"a, b,,c"` shouldn't produce 4 probe
        // entries. Trim each entry; skip empties.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "  bogus-a  , bogus-b ,, ");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 2);
            assert_eq!(probed[0].name, "bogus-a");
            assert_eq!(probed[1].name, "bogus-b");
        } else {
            panic!("Expected NoBackendFound error from whitespace-padded list");
        }

        // -------------------------------------------------------------
        // Phase 4: empty string falls through to platform default
        // -------------------------------------------------------------
        // `PERRY_CONTAINER_BACKEND= ./app` is a real shell idiom for
        // "clear an override inherited from the parent env." It
        // shouldn't error; should behave as if the var was unset.
        std::env::set_var("PERRY_CONTAINER_BACKEND", "");
        let res = detect_backend().await;
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        // Can't assert Ok vs Err deterministically (depends on test
        // runner's installed runtimes), but if Err, the probed list
        // length must match platform_candidates, NOT 0 (which would
        // mean the empty-list path was taken).
        if let Err(ComposeError::NoBackendFound { probed }) = res {
            let candidates = platform_candidates();
            assert_eq!(
                probed.len(),
                candidates.len(),
                "empty env var should fall through to platform_candidates probe"
            );
        }
    }
}

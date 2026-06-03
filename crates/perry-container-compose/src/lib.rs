//! `perry-container-compose` — Docker Compose-like experience for Apple Container / Podman.

pub mod backend;
pub mod capabilities;
pub mod cli;
pub mod compose;
pub mod config;
pub mod error;
pub mod installer;
pub mod orchestrate;
pub mod project;
pub mod service;
pub mod types;
pub mod workload;
pub mod yaml;

// `commands/` is a legacy/dead module from an earlier `ContainerCommand`
// trait shape. The functionality is now covered by the per-method
// orchestration on `ComposeService` (`run_command`/`start_command`/
// `build_command`/`inspect_command`) plus `orchestrate::orchestrate_service`
// for the single-service flow. Files are retained on disk as historical
// reference but are *not* compiled into the crate.

#[cfg(any(test, feature = "test-utils"))]
pub mod testing;

// FFI exports (Perry TypeScript integration). NOTE: when this crate is
// consumed by perry-stdlib (the canonical FFI host), the `ffi` feature
// must NOT be enabled — perry-stdlib publishes a different (canonical
// SPEC §9.1, stack-handle based) `js_compose_*` shape that would collide
// at link with this module's legacy YAML-file-path shape.
#[cfg(feature = "ffi")]
pub mod ffi;

// Re-exports
pub use backend::{
    detect_backend, platform_candidates, probe_all_candidates, AppleContainerProtocol,
    BackendProbeResult, CliBackend, CliProtocol, ContainerBackend, DockerProtocol, LimaProtocol,
};
pub use capabilities::{
    capabilities_for_backend, normalise_security_profile, normalise_spec_for, required_features,
    select_backend_for, unsupported_feature_names, BackendCapabilities, EnforcementMode,
    FeatureSupport, NormalizationAction, NormalizationWarning, SelectMode,
};
pub use compose::{resolve_startup_order, ComposeEngine};
pub use error::{ComposeError, Result};
pub use indexmap;
pub use installer::BackendInstaller;
pub use project::ComposeProject;
pub use types::{ComposeHandle, ComposeService, ComposeSpec};
pub use workload::{
    get_workload_engine, register_workload_engine, ExecutionStrategy, FailureStrategy, PolicySpec,
    PolicyTier, RunGraphOptions, RuntimeSpec, WorkloadEdge, WorkloadEnvValue, WorkloadGraph,
    WorkloadGraphEngine, WorkloadNode, WorkloadRef,
};

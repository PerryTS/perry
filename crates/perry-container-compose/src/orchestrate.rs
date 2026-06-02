//! Per-service orchestration helper (Task 0.4 in the implementation plan).
//!
//! Mirrors the canonical container-compose Go reference (`cmd/start/cmd.go`)
//! decision flow:
//!
//! 1. If the service's container is already running → skip (idempotent up).
//! 2. If it exists but is stopped → `start_command`.
//! 3. If it doesn't exist → optionally `build_command` (when `needs_build()`)
//!    then `run_command`.
//!
//! `ComposeEngine::up` inlines an equivalent flow for the multi-service path
//! because it tracks per-session resources (containers/networks/volumes) for
//! rollback. This module exposes the same logic for **single-service**
//! callers that don't need the full session bookkeeping (e.g. the standalone
//! CLI's `perry-compose run <service>` path or programmatic per-service
//! restart).

use crate::backend::ContainerBackend;
use crate::error::Result;
use crate::types::{ComposeService, ContainerHandle};

/// Orchestrate a single service startup. Returns the container handle when a
/// fresh container was created or `Ok(None)` when the service was already
/// running OR was a stopped-existing container that we just `start`ed (the
/// backend doesn't return a handle from a bare `start`).
pub async fn orchestrate_service(
    service: &ComposeService,
    service_name: &str,
    backend: &dyn ContainerBackend,
) -> Result<Option<ContainerHandle>> {
    if service.is_running(backend, service_name).await? {
        tracing::info!(service = %service_name, "already running, skipping");
        return Ok(None);
    }

    if service.exists(backend, service_name).await? {
        tracing::info!(service = %service_name, "exists but stopped, starting");
        service.start_command(backend, service_name).await?;
        return Ok(None);
    }

    if service.needs_build() {
        tracing::info!(service = %service_name, "building image");
        service.build_command(backend, service_name).await?;
    }
    tracing::info!(service = %service_name, "creating and running");
    let handle = service.run_command(backend, service_name).await?;
    Ok(Some(handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::mock_backend::{MockBackend, RecordedCall};
    use crate::types::{ComposeService, ComposeServiceBuild};

    fn svc_with_image(image: &str) -> ComposeService {
        ComposeService {
            image: Some(image.to_string()),
            ..Default::default()
        }
    }

    fn svc_with_build(context: &str) -> ComposeService {
        ComposeService {
            build: Some(crate::types::BuildSpec::Config(ComposeServiceBuild {
                context: Some(context.to_string()),
                ..Default::default()
            })),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn already_running_skips_orchestration() {
        let mock = MockBackend::new();
        // Default: any inspect returns running info → is_running = true.
        mock.set_inspect_running(true).await;
        let svc = svc_with_image("alpine");
        let result = orchestrate_service(&svc, "web", &mock).await.unwrap();
        assert!(
            matches!(result, None),
            "running service should skip and return None"
        );
        let calls = mock.calls().await;
        assert!(
            !calls.iter().any(|c| matches!(c, RecordedCall::Run { .. })),
            "running service must not call run"
        );
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, RecordedCall::Start { .. })),
            "running service must not call start"
        );
    }

    #[tokio::test]
    async fn stopped_existing_service_is_started_not_run() {
        let mock = MockBackend::new();
        mock.set_inspect_running(false).await;
        let svc = svc_with_image("alpine");
        let result = orchestrate_service(&svc, "web", &mock).await.unwrap();
        assert!(
            matches!(result, None),
            "start path returns None (no fresh handle)"
        );
        let calls = mock.calls().await;
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, RecordedCall::Start { .. })),
            "expected backend.start to be called"
        );
        assert!(
            !calls.iter().any(|c| matches!(c, RecordedCall::Run { .. })),
            "stopped+existing path must not call run"
        );
    }

    #[tokio::test]
    async fn missing_service_with_build_calls_build_then_run() {
        let mock = MockBackend::new();
        mock.set_inspect_not_found().await;
        let svc = svc_with_build(".");
        let result = orchestrate_service(&svc, "api", &mock).await.unwrap();
        assert!(matches!(result, Some(_)), "fresh run returns a handle");
        let calls = mock.calls().await;
        let build_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::Build { .. }))
            .expect("expected backend.build");
        let run_idx = calls
            .iter()
            .position(|c| matches!(c, RecordedCall::Run { .. }))
            .expect("expected backend.run");
        assert!(
            build_idx < run_idx,
            "build must precede run (Task 0.4 ordering invariant)"
        );
    }

    #[tokio::test]
    async fn missing_service_no_build_skips_build() {
        let mock = MockBackend::new();
        mock.set_inspect_not_found().await;
        let svc = svc_with_image("alpine"); // image set, no build
        let _ = orchestrate_service(&svc, "cache", &mock).await.unwrap();
        let calls = mock.calls().await;
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, RecordedCall::Build { .. })),
            "service without build field must not call build"
        );
        assert!(
            calls.iter().any(|c| matches!(c, RecordedCall::Run { .. })),
            "missing-image service should call run"
        );
    }
}

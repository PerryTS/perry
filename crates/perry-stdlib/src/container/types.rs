//! Type definitions for the perry/container module.

use dashmap::DashMap;
use perry_runtime::StringHeader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use perry_container_compose::ComposeEngine;

// ============ Handle Registry ============

pub static CONTAINER_HANDLES: OnceLock<DashMap<u64, ContainerHandle>> = OnceLock::new();
pub static COMPOSE_HANDLES: OnceLock<DashMap<u64, ArcComposeEngine>> = OnceLock::new();
pub static WORKLOAD_HANDLES: OnceLock<
    DashMap<u64, std::sync::Arc<perry_container_compose::WorkloadGraphEngine>>,
> = OnceLock::new();

pub static CONTAINER_INFO_LIST_REGISTRY: OnceLock<DashMap<u64, Vec<ContainerInfo>>> =
    OnceLock::new();
pub static CONTAINER_INFO_REGISTRY: OnceLock<DashMap<u64, ContainerInfo>> = OnceLock::new();
pub static CONTAINER_LOGS_REGISTRY: OnceLock<DashMap<u64, ContainerLogs>> = OnceLock::new();
pub static IMAGE_INFO_LIST_REGISTRY: OnceLock<DashMap<u64, Vec<ImageInfo>>> = OnceLock::new();

pub static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

pub struct ArcComposeEngine(pub std::sync::Arc<ComposeEngine>);

pub type ContainerError = perry_container_compose::error::ComposeError;
pub use perry_container_compose::types::{ComposeSpec, ListOrDict};

pub unsafe fn parse_container_spec(
    ptr: *const perry_runtime::StringHeader,
) -> Result<ContainerSpec, String> {
    let json = string_from_header(ptr).ok_or("Invalid JSON")?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

pub unsafe fn parse_compose_spec(
    ptr: *const perry_runtime::StringHeader,
) -> Result<perry_container_compose::types::ComposeSpec, String> {
    let json = string_from_header(ptr).ok_or("Invalid JSON")?;
    // Apply env-var interpolation (`${VAR}` / `${VAR:-default}`) BEFORE
    // JSON parsing — the spec from TS object literals carries placeholder
    // strings verbatim (e.g. POSTGRES_USER=`${FORGEJO_DB_USER:-forgejo}`),
    // and the FFI is the canonical interpolation point per SPEC §7.8 / §7.9.
    // Pre-fix postgres rejected the literal `$`-prefixed username with
    // "FATAL: invalid character in extension owner".
    let env: HashMap<String, String> = std::env::vars().collect();
    let interpolated = perry_container_compose::yaml::interpolate(&json, &env);
    serde_json::from_str(&interpolated).map_err(|e| e.to_string())
}

pub fn take_compose_handle(id: u64) -> Option<std::sync::Arc<ComposeEngine>> {
    COMPOSE_HANDLES.get()?.remove(&id).map(|(_, arc)| arc.0)
}

pub fn get_compose_handle(id: u64) -> Option<std::sync::Arc<ComposeEngine>> {
    COMPOSE_HANDLES.get()?.get(&id).map(|arc| arc.0.clone())
}

pub fn register_container_info_list(list: Vec<ContainerInfo>) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    CONTAINER_INFO_LIST_REGISTRY
        .get_or_init(DashMap::new)
        .insert(id, list);
    id
}

pub fn register_container_info(info: ContainerInfo) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    CONTAINER_INFO_REGISTRY
        .get_or_init(DashMap::new)
        .insert(id, info);
    id
}

pub fn register_container_logs(logs: ContainerLogs) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    CONTAINER_LOGS_REGISTRY
        .get_or_init(DashMap::new)
        .insert(id, logs);
    id
}

pub fn register_image_info_list(list: Vec<ImageInfo>) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    IMAGE_INFO_LIST_REGISTRY
        .get_or_init(DashMap::new)
        .insert(id, list);
    id
}

pub fn register_container_handle(handle: ContainerHandle) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    CONTAINER_HANDLES
        .get_or_init(DashMap::new)
        .insert(id, handle);
    id
}

pub fn register_compose_handle(engine: std::sync::Arc<ComposeEngine>) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    COMPOSE_HANDLES
        .get_or_init(DashMap::new)
        .insert(id, ArcComposeEngine(engine));
    id
}

pub fn register_workload_handle(
    engine: std::sync::Arc<perry_container_compose::WorkloadGraphEngine>,
) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    WORKLOAD_HANDLES
        .get_or_init(DashMap::new)
        .insert(id, engine);
    id
}

// ============ Core Container Types ============

pub use perry_container_compose::types::{
    ComposeHandle, ContainerHandle, ContainerInfo, ContainerLogs, ContainerSpec, ImageInfo,
};

// ============ Helper for StringHeader ============

pub unsafe fn string_from_header(header: *const StringHeader) -> Option<String> {
    if header.is_null() || (header as usize) < 0x1000 {
        return None;
    }
    let byte_len = (*header).byte_len as usize;
    let data_ptr = (header as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, byte_len);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

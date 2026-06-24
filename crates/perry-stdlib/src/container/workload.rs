use super::*;

pub use types::{
    ComposeHandle, ComposeSpec, ContainerError, ContainerHandle, ContainerInfo, ContainerLogs,
    ContainerSpec, ImageInfo, ListOrDict,
};

pub use backend::{detect_backend, ContainerBackend};
use perry_runtime::{js_promise_new, Promise, StringHeader};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

// ============ Workload Functions ============

/// Create a workload graph
/// FFI: js_workload_graph(name: *const StringHeader, nodes_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_graph(
    name_ptr: *const StringHeader,
    nodes_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let nodes_json = string_from_header(nodes_json_ptr).unwrap_or_else(|| "{}".to_string());

    let graph = perry_container_compose::WorkloadGraph {
        name,
        nodes: serde_json::from_str(&nodes_json).unwrap_or_default(),
        edges: vec![], // Edges inferred from depends_on in nodes
    };

    let json = serde_json::to_string(&graph).unwrap_or_default();
    string_to_js(&json)
}

/// Create a workload node
/// FFI: js_workload_node(name: *const StringHeader, spec_json: *const StringHeader) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_node(
    name_ptr: *const StringHeader,
    spec_json_ptr: *const StringHeader,
) -> *const StringHeader {
    let name = string_from_header(name_ptr).unwrap_or_default();
    let spec_json = string_from_header(spec_json_ptr).unwrap_or_else(|| "{}".to_string());

    let mut node: perry_container_compose::WorkloadNode = serde_json::from_str(&spec_json)
        .unwrap_or_else(|_| perry_container_compose::WorkloadNode {
            id: name.clone(),
            name: name.clone(),
            image: None,
            resources: None,
            ports: vec![],
            env: HashMap::new(),
            depends_on: vec![],
            runtime: perry_container_compose::RuntimeSpec::Auto,
            policy: perry_container_compose::PolicySpec::default(),
        });
    node.id = name.clone();
    node.name = name;

    let json = serde_json::to_string(&node).unwrap_or_default();
    string_to_js(&json)
}

/// Run a workload graph
/// FFI: js_workload_runGraph(graph_json: *const StringHeader, opts_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_runGraph(
    graph_json_ptr: *const StringHeader,
    opts_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();

    let graph_json = string_from_header(graph_json_ptr).unwrap_or_else(|| "{}".to_string());
    let opts_json = string_from_header(opts_json_ptr).unwrap_or_else(|| "{}".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let graph: perry_container_compose::WorkloadGraph = serde_json::from_str(&graph_json)
            .map_err(|e| format!("Failed to parse graph: {}", e))?;
        let opts: perry_container_compose::RunGraphOptions = serde_json::from_str(&opts_json)
            .map_err(|e| format!("Failed to parse options: {}", e))?;

        let backend = match get_global_backend().await {
            Ok(b) => Arc::clone(b),
            Err(e) => return Err::<u64, String>(e.to_string()),
        };

        let engine = Arc::new(perry_container_compose::WorkloadGraphEngine::new(
            graph, backend,
        ));
        match engine.run(opts).await {
            Ok(_) => {
                let handle_id = types::register_workload_handle(engine);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Inspect a workload graph
/// FFI: js_workload_inspectGraph(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_inspectGraph(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Stop and remove a workload graph
/// FFI: js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_down(handle_id: i64, force: i32) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.down(force != 0).await {
            Ok(_) => {
                if let Some(handles) = types::WORKLOAD_HANDLES.get() {
                    handles.remove(&id);
                }
                Ok(PROMISE_VOID_BITS)
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get status of a workload graph
/// FFI: js_workload_handle_status(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_status(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
                Some(e) => e.clone(),
                None => return Err("Invalid workload handle".to_string()),
            };

            match engine.status().await {
                Ok(status) => {
                    let json = serde_json::to_string(&status).unwrap_or_default();
                    Ok(json)
                }
                Err(e) => Err(e.to_string()),
            }
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            perry_runtime::JSValue::string_ptr(str_ptr).bits()
        },
    );

    promise
}

/// Get logs from a workload node
/// FFI: js_workload_handle_logs(handle_id: i64, node_id: *const StringHeader, tail: i32) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_logs(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    tail: i32,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let tail_opt = if tail >= 0 { Some(tail as u32) } else { None };

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.logs(&node_id, tail_opt).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Execute command in a workload node
/// FFI: js_workload_handle_exec(handle_id: i64, node_id: *const StringHeader, cmd_json: *const StringHeader) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_exec(
    handle_id: i64,
    node_id_ptr: *const StringHeader,
    cmd_json_ptr: *const StringHeader,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;
    let node_id = string_from_header(node_id_ptr).unwrap_or_default();
    let cmd_json = string_from_header(cmd_json_ptr).unwrap_or_else(|| "[]".to_string());

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let cmd: Vec<String> = serde_json::from_str(&cmd_json).unwrap_or_default();
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.exec(&node_id, &cmd).await {
            Ok(logs) => {
                let handle_id = types::register_container_logs(logs);
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get process status of a workload graph
/// FFI: js_workload_handle_ps(handle_id: i64) -> *mut Promise
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_ps(handle_id: i64) -> *mut Promise {
    let promise = js_promise_new();
    let id = handle_id as u64;

    crate::common::spawn_for_promise(promise as *mut u8, async move {
        let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
            Some(e) => e.clone(),
            None => return Err("Invalid workload handle".to_string()),
        };

        match engine.ps().await {
            Ok(infos) => {
                // Register NodeInfo list as a container info list (compatible for now)
                // Actually we should probably have a register_node_info_list
                let handle_id = types::register_container_info_list(
                    infos
                        .into_iter()
                        .map(|i| ContainerInfo {
                            id: i.container_id.unwrap_or_default(),
                            name: i.name,
                            image: i.image.unwrap_or_default(),
                            status: format!("{:?}", i.state),
                            ports: vec![],
                            labels: HashMap::new(),
                            created: "".to_string(),
                            ip_address: i.ip_address.unwrap_or_default(),
                        })
                        .collect(),
                );
                Ok(handle_to_promise_bits(handle_id))
            }
            Err(e) => Err::<u64, String>(e.to_string()),
        }
    });

    promise
}

/// Get graph JSON from workload handle
/// FFI: js_workload_handle_graph(handle_id: i64) -> *const StringHeader
#[no_mangle]
pub unsafe extern "C" fn js_workload_handle_graph(handle_id: i64) -> *const StringHeader {
    let id = handle_id as u64;
    let engine = match types::WORKLOAD_HANDLES.get().and_then(|m| m.get(&id)) {
        Some(e) => e.clone(),
        None => return std::ptr::null(),
    };

    let json = serde_json::to_string(&engine.graph).unwrap_or_default();
    string_to_js(&json)
}

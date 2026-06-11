//! Net module - provides TCP networking capabilities

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::buffer::BufferHeader;
use crate::closure::ClosureHeader;
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};

// Global handle registry for servers and sockets
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref TCP_SERVERS: Mutex<HashMap<u64, TcpListenerWrapper>> = Mutex::new(HashMap::new());
    static ref TCP_SOCKETS: Mutex<HashMap<u64, TcpStreamWrapper>> = Mutex::new(HashMap::new());
}

struct TcpListenerWrapper {
    listener: TcpListener,
    address: SocketAddr,
}

struct TcpStreamWrapper {
    stream: TcpStream,
    remote_address: Option<SocketAddr>,
}

/// Create a TCP server
/// Returns a handle (u64 as f64) to the server
#[no_mangle]
pub extern "C" fn js_net_create_server(
    _options_ptr: *const ObjectHeader,
    _connection_listener_ptr: *const ClosureHeader,
) -> f64 {
    // For now, just return a placeholder handle
    // Real server creation happens in listen()
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    handle as f64
}

/// Start a TCP server listening on a port
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub extern "C" fn js_net_server_listen(
    handle: f64,
    port: i32,
    host_ptr: *const StringHeader,
    _callback_ptr: *const ClosureHeader,
) -> i32 {
    let handle = handle as u64;

    // Get host string (default to 0.0.0.0)
    let host = if host_ptr.is_null() {
        "0.0.0.0".to_string()
    } else {
        unsafe {
            let len = (*host_ptr).byte_len as usize;
            let data = (host_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            String::from_utf8_lossy(bytes).to_string()
        }
    };

    let addr = format!("{}:{}", host, port);
    // #4914 — cluster workers bind with SO_REUSEPORT so N workers share the
    // port, then report the bound address to the primary for 'listening'.
    #[cfg(unix)]
    let bind_result = if crate::cluster::is_cluster_worker() {
        crate::cluster::worker_reuseport_bind(&addr)
    } else {
        TcpListener::bind(&addr)
    };
    #[cfg(not(unix))]
    let bind_result = TcpListener::bind(&addr);
    match bind_result {
        Ok(listener) => {
            let address = listener
                .local_addr()
                .unwrap_or_else(|_| addr.parse().unwrap());
            let address_type: i32 = if address.is_ipv6() { 6 } else { 4 };
            crate::cluster::perry_cluster_worker_listening(
                host.as_ptr(),
                host.len() as u32,
                address.port() as i32,
                address_type,
            );
            let wrapper = TcpListenerWrapper { listener, address };

            if let Ok(mut servers) = TCP_SERVERS.lock() {
                servers.insert(handle, wrapper);
                1
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

/// Close a TCP server
#[no_mangle]
pub extern "C" fn js_net_server_close(handle: f64) -> i32 {
    let handle = handle as u64;
    if let Ok(mut servers) = TCP_SERVERS.lock() {
        servers.remove(&handle);
        1
    } else {
        0
    }
}

/// Get server address info
/// Returns an object with port, family, address
#[no_mangle]
pub extern "C" fn js_net_server_address(handle: f64) -> *mut ObjectHeader {
    let handle = handle as u64;

    if let Ok(servers) = TCP_SERVERS.lock() {
        if let Some(wrapper) = servers.get(&handle) {
            // Create object with 3 fields: port, address, family
            let obj = crate::object::js_object_alloc(0, 3);

            unsafe {
                // Set port (field 0)
                crate::object::js_object_set_field_f64(obj, 0, wrapper.address.port() as f64);

                // Set address (field 1)
                let addr_str = wrapper.address.ip().to_string();
                let addr_val = js_string_from_bytes(addr_str.as_ptr(), addr_str.len() as u32);
                crate::object::js_object_set_field_f64(obj, 1, (addr_val as u64) as f64);

                // Set family (field 2)
                let family = if wrapper.address.is_ipv4() {
                    "IPv4"
                } else {
                    "IPv6"
                };
                let family_val = js_string_from_bytes(family.as_ptr(), family.len() as u32);
                crate::object::js_object_set_field_f64(obj, 2, (family_val as u64) as f64);
            }

            return obj;
        }
    }

    std::ptr::null_mut()
}

/// Create a TCP connection to a server
/// Returns a socket handle
#[no_mangle]
pub extern "C" fn js_net_create_connection(
    port: i32,
    host_ptr: *const StringHeader,
    _connect_listener_ptr: *const ClosureHeader,
) -> f64 {
    // Get host string (default to localhost)
    let host = if host_ptr.is_null() {
        "127.0.0.1".to_string()
    } else {
        unsafe {
            let len = (*host_ptr).byte_len as usize;
            let data = (host_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            String::from_utf8_lossy(bytes).to_string()
        }
    };

    let addr = format!("{}:{}", host, port);
    match TcpStream::connect(&addr) {
        Ok(stream) => {
            let remote_address = stream.peer_addr().ok();
            let handle = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
            let wrapper = TcpStreamWrapper {
                stream,
                remote_address,
            };

            if let Ok(mut sockets) = TCP_SOCKETS.lock() {
                sockets.insert(handle, wrapper);
                return handle as f64;
            }
            0.0
        }
        Err(_) => 0.0,
    }
}

/// Write data to a socket
/// Returns the number of bytes written, or -1 on error
#[no_mangle]
pub extern "C" fn js_net_socket_write(handle: f64, data_ptr: *const BufferHeader) -> i32 {
    let handle = handle as u64;

    if data_ptr.is_null() {
        return -1;
    }

    if let Ok(mut sockets) = TCP_SOCKETS.lock() {
        if let Some(wrapper) = sockets.get_mut(&handle) {
            unsafe {
                let len = (*data_ptr).length as usize;
                let data = (data_ptr as *const u8).add(std::mem::size_of::<BufferHeader>());
                let bytes = std::slice::from_raw_parts(data, len);

                match wrapper.stream.write(bytes) {
                    Ok(n) => return n as i32,
                    Err(_) => return -1,
                }
            }
        }
    }

    -1
}

/// Read data from a socket
/// Returns a buffer with the read data, or null on error
#[no_mangle]
pub extern "C" fn js_net_socket_read(handle: f64, max_size: i32) -> *mut BufferHeader {
    let handle = handle as u64;
    let max_size = max_size.max(0) as usize;

    if let Ok(mut sockets) = TCP_SOCKETS.lock() {
        if let Some(wrapper) = sockets.get_mut(&handle) {
            let mut buffer = vec![0u8; max_size.min(65536)];

            match wrapper.stream.read(&mut buffer) {
                Ok(n) => {
                    let buf = crate::buffer::js_buffer_alloc(n as i32, 0);
                    if !buf.is_null() {
                        unsafe {
                            let buf_data =
                                (buf as *mut u8).add(std::mem::size_of::<BufferHeader>());
                            std::ptr::copy_nonoverlapping(buffer.as_ptr(), buf_data, n);
                            (*buf).length = n as u32;
                        }
                    }
                    return buf;
                }
                Err(_) => return std::ptr::null_mut(),
            }
        }
    }

    std::ptr::null_mut()
}

/// End a socket connection
#[no_mangle]
pub extern "C" fn js_net_socket_end(handle: f64) -> i32 {
    let handle = handle as u64;

    if let Ok(mut sockets) = TCP_SOCKETS.lock() {
        if let Some(wrapper) = sockets.get_mut(&handle) {
            let _ = wrapper.stream.shutdown(std::net::Shutdown::Write);
            return 1;
        }
    }

    0
}

/// Destroy a socket connection
#[no_mangle]
pub extern "C" fn js_net_socket_destroy(handle: f64) -> i32 {
    let handle = handle as u64;

    if let Ok(mut sockets) = TCP_SOCKETS.lock() {
        sockets.remove(&handle);
        return 1;
    }

    0
}

/// Get the remote address of a socket
#[no_mangle]
pub extern "C" fn js_net_socket_remote_address(handle: f64) -> *mut StringHeader {
    let handle = handle as u64;

    if let Ok(sockets) = TCP_SOCKETS.lock() {
        if let Some(wrapper) = sockets.get(&handle) {
            if let Some(addr) = &wrapper.remote_address {
                let addr_str = addr.ip().to_string();
                return js_string_from_bytes(addr_str.as_ptr(), addr_str.len() as u32);
            }
        }
    }

    std::ptr::null_mut()
}

/// Get the remote port of a socket
#[no_mangle]
pub extern "C" fn js_net_socket_remote_port(handle: f64) -> i32 {
    let handle = handle as u64;

    if let Ok(sockets) = TCP_SOCKETS.lock() {
        if let Some(wrapper) = sockets.get(&handle) {
            if let Some(addr) = &wrapper.remote_address {
                return addr.port() as i32;
            }
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_server() {
        let handle = js_net_create_server(std::ptr::null(), std::ptr::null());
        assert!(handle > 0.0);
    }
}

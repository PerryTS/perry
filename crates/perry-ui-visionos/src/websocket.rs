//! Native iOS WebSocket using NSURLSessionWebSocketTask.
//!
//! Bypasses tokio (which doesn't work on iOS) and uses Apple's
//! native networking stack instead.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, define_class, DefinedClass, AnyThread};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Per-connection state — stores raw pointers to avoid objc2 Retained issues
struct WsConn {
    /// Raw pointer to NSURLSessionWebSocketTask (retained via CFRetain)
    task: *const AnyObject,
    /// Raw pointer to NSURLSession (retained via CFRetain)
    session: *const AnyObject,
    is_open: bool,
    messages: Vec<String>,
}

// Safety: WsConn is only accessed from the main thread (thread_local)
unsafe impl Send for WsConn {}

thread_local! {
    static CONNECTIONS: RefCell<HashMap<u32, WsConn>> = RefCell::new(HashMap::new());
    static NEXT_ID: RefCell<u32> = RefCell::new(1);
}

extern "C" {
    fn CFRetain(cf: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// Delegate that receives open/close WebSocket events.
pub struct WsDelegateIvars {
    conn_id: std::cell::Cell<u32>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryWsDelegate"]
    #[ivars = WsDelegateIvars]
    pub struct PerryWsDelegate;

    impl PerryWsDelegate {
        #[unsafe(method(URLSession:webSocketTask:didOpenWithProtocol:))]
        fn did_open(&self, _session: &AnyObject, _task: &AnyObject, _protocol: Option<&NSString>) {
            let cid = self.ivars().conn_id.get();
            crate::ws_log!("[WS-iOS] didOpen for conn_id={}", cid);
            CONNECTIONS.with(|conns| {
                if let Some(conn) = conns.borrow_mut().get_mut(&cid) {
                    conn.is_open = true;
                    // Start receiving messages now that we're connected
                    schedule_receive_raw(cid, conn.task);
                }
            });
        }

        #[unsafe(method(URLSession:webSocketTask:didCloseWithCode:reason:))]
        fn did_close(&self, _session: &AnyObject, _task: &AnyObject, _code: i64, _reason: Option<&AnyObject>) {
            let cid = self.ivars().conn_id.get();
            crate::ws_log!("[WS-iOS] didClose for conn_id={}, code={}", cid, _code);
            CONNECTIONS.with(|conns| {
                if let Some(conn) = conns.borrow_mut().get_mut(&cid) {
                    conn.is_open = false;
                }
            });
        }

        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn did_complete(&self, _session: &AnyObject, _task: &AnyObject, error: Option<&AnyObject>) {
            let cid = self.ivars().conn_id.get();
            if let Some(err) = error {
                let desc: *const NSString = unsafe { msg_send![err, localizedDescription] };
                if !desc.is_null() {
                    let msg = unsafe { (*desc).to_string() };
                    crate::ws_log!("[WS-iOS] didCompleteWithError conn_id={}: {}", cid, msg);
                } else {
                    crate::ws_log!("[WS-iOS] didCompleteWithError conn_id={}: (no description)", cid);
                }
            } else {
                crate::ws_log!("[WS-iOS] didComplete conn_id={} (no error)", cid);
            }
            CONNECTIONS.with(|conns| {
                if let Some(conn) = conns.borrow_mut().get_mut(&cid) {
                    conn.is_open = false;
                }
            });
        }
    }
);

impl PerryWsDelegate {
    fn new(conn_id: u32) -> Retained<Self> {
        let this = Self::alloc().set_ivars(WsDelegateIvars {
            conn_id: std::cell::Cell::new(conn_id),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Schedule receiving one message, then re-schedule on success.
fn schedule_receive_raw(conn_id: u32, task_ptr: *const AnyObject) {
    crate::ws_log!("[WS-iOS] schedule_receive_raw conn_id={} task_null={}", conn_id, task_ptr.is_null());
    if task_ptr.is_null() {
        return;
    }
    unsafe {
        let block = block2::RcBlock::new(move |message: *const AnyObject, error: *const AnyObject| {
            if !error.is_null() || message.is_null() {
                // Error or nil message — connection likely closed
                if !error.is_null() {
                    let desc: *const NSString = msg_send![error, localizedDescription];
                    if !desc.is_null() {
                        crate::ws_log!("[WS-iOS] recv error conn_id={}: {}", conn_id, (*desc).to_string());
                    }
                } else {
                    crate::ws_log!("[WS-iOS] recv nil message conn_id={}", conn_id);
                }
                CONNECTIONS.with(|conns| {
                    if let Some(conn) = conns.borrow_mut().get_mut(&conn_id) {
                        conn.is_open = false;
                    }
                });
                return;
            }
            // NSURLSessionWebSocketMessage: type 0=data, 1=string
            let msg_type: i64 = msg_send![message, type];
            if msg_type == 1 {
                let ns_string: *const NSString = msg_send![message, string];
                if !ns_string.is_null() {
                    let rust_string = (*ns_string).to_string();
                    crate::ws_log!("[WS-iOS] recv conn_id={} len={} first80={}", conn_id, rust_string.len(), &rust_string[..rust_string.len().min(80)]);
                    CONNECTIONS.with(|conns| {
                        if let Some(conn) = conns.borrow_mut().get_mut(&conn_id) {
                            conn.messages.push(rust_string);
                        }
                    });
                }
            }
            // Re-schedule next receive
            let still_open = CONNECTIONS.with(|conns| {
                conns.borrow().get(&conn_id).map(|c| c.is_open).unwrap_or(false)
            });
            crate::ws_log!("[WS-iOS] re-schedule? still_open={}", still_open);
            if still_open {
                schedule_receive_raw(conn_id, task_ptr);
            } else {
                crate::ws_log!("[WS-iOS] NOT re-scheduling, conn closed");
            }
        });
        let _: () = msg_send![task_ptr, receiveMessageWithCompletionHandler: &*block];
    }
}

/// Connect to a WebSocket URL. Returns a connection handle (f64).
pub fn connect(url_ptr: *const u8) -> f64 {
    let url_str = str_from_header(url_ptr);
    crate::ws_log!("[WS-iOS] connect called, url={}", url_str);
    if url_str.is_empty() {
        crate::ws_log!("[WS-iOS] empty URL, returning 0");
        return 0.0;
    }

    let conn_id = NEXT_ID.with(|id| {
        let current = *id.borrow();
        *id.borrow_mut() = current + 1;
        current
    });
    crate::ws_log!("[WS-iOS] conn_id={}", conn_id);

    unsafe {
        let ns_url_str = NSString::from_str(url_str);
        let url_cls = AnyClass::get(c"NSURL").unwrap();
        let url: *const AnyObject = msg_send![url_cls, URLWithString: &*ns_url_str];
        if url.is_null() {
            crate::ws_log!("[WS-iOS] NSURL is null!");
            return 0.0;
        }
        CFRetain(url as *const _);

        // Create delegate
        let delegate = PerryWsDelegate::new(conn_id);

        // Create NSURLSession with delegate
        let config_cls = AnyClass::get(c"NSURLSessionConfiguration").unwrap();
        let config: *const AnyObject = msg_send![config_cls, defaultSessionConfiguration];

        let session_cls = AnyClass::get(c"NSURLSession").unwrap();
        // MUST use mainQueue so delegate callbacks fire on the main thread,
        // where thread_local CONNECTIONS is accessible.
        let op_queue_cls = AnyClass::get(c"NSOperationQueue").unwrap();
        let queue: *const AnyObject = msg_send![op_queue_cls, mainQueue];
        let session: *const AnyObject = msg_send![
            session_cls,
            sessionWithConfiguration: config,
            delegate: &*delegate,
            delegateQueue: queue
        ];
        CFRetain(session as *const _);

        // Create WebSocket task
        let task: *const AnyObject = msg_send![session, webSocketTaskWithURL: url];
        CFRetain(task as *const _);

        // Resume (start connecting)
        let _: () = msg_send![task, resume];

        // Store connection
        CONNECTIONS.with(|conns| {
            conns.borrow_mut().insert(conn_id, WsConn {
                task,
                session,
                is_open: false,
                messages: Vec::new(),
            });
        });

        // Keep delegate alive
        std::mem::forget(delegate);

        // Release our local NSURL ref
        CFRelease(url as *const _);

        conn_id as f64
    }
}

/// Check if a WebSocket connection is open.
pub fn is_open(handle: f64) -> f64 {
    let conn_id = handle as u32;
    CONNECTIONS.with(|conns| {
        match conns.borrow().get(&conn_id) {
            Some(conn) if conn.is_open => 1.0,
            _ => 0.0,
        }
    })
}

/// Get pending message count.
pub fn message_count(handle: f64) -> f64 {
    let conn_id = handle as u32;
    CONNECTIONS.with(|conns| {
        match conns.borrow().get(&conn_id) {
            Some(conn) => conn.messages.len() as f64,
            None => 0.0,
        }
    })
}

/// Receive the next queued message. Returns a NaN-boxed string.
pub fn receive(handle: f64) -> f64 {
    let conn_id = handle as u32;
    CONNECTIONS.with(|conns| {
        if let Some(conn) = conns.borrow_mut().get_mut(&conn_id) {
            if !conn.messages.is_empty() {
                let msg = conn.messages.remove(0);
                let bytes = msg.as_bytes();
                unsafe {
                    let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                    return js_nanbox_string(str_ptr as i64);
                }
            }
        }
        unsafe {
            let str_ptr = js_string_from_bytes(std::ptr::null(), 0);
            js_nanbox_string(str_ptr as i64)
        }
    })
}

/// Send a string message.
pub fn send(handle: f64, msg_ptr: *const u8) {
    let conn_id = handle as u32;
    let msg_str = str_from_header(msg_ptr);
    if msg_str.is_empty() {
        return;
    }
    CONNECTIONS.with(|conns| {
        if let Some(conn) = conns.borrow().get(&conn_id) {
            if conn.is_open && !conn.task.is_null() {
                unsafe {
                    let ns_string = NSString::from_str(msg_str);
                    let msg_cls = AnyClass::get(c"NSURLSessionWebSocketMessage").unwrap();
                    let ws_msg: *const AnyObject = msg_send![msg_cls, alloc];
                    let ws_msg: *const AnyObject = msg_send![ws_msg, initWithString: &*ns_string];

                    let block = block2::RcBlock::new(|_error: *const AnyObject| {});
                    let _: () = msg_send![
                        conn.task,
                        sendMessage: ws_msg,
                        completionHandler: &*block
                    ];
                }
            }
        }
    });
}

// =============================================================================
// perry_native_ws_* exports — called by perry-stdlib/ws.rs on iOS via extern "C"
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_native_ws_connect(url_ptr: *const u8) -> f64 {
    connect(url_ptr)
}

#[no_mangle]
pub extern "C" fn perry_native_ws_is_open(handle: f64) -> f64 {
    is_open(handle)
}

#[no_mangle]
pub extern "C" fn perry_native_ws_send(handle: f64, msg_ptr: *const u8) {
    send(handle, msg_ptr)
}

#[no_mangle]
pub extern "C" fn perry_native_ws_receive(handle: f64) -> f64 {
    receive(handle)
}

#[no_mangle]
pub extern "C" fn perry_native_ws_message_count(handle: f64) -> f64 {
    message_count(handle)
}

#[no_mangle]
pub extern "C" fn perry_native_ws_close(handle: f64) {
    close(handle)
}

/// Close the WebSocket connection.
pub fn close(handle: f64) {
    let conn_id = handle as u32;
    CONNECTIONS.with(|conns| {
        if let Some(conn) = conns.borrow_mut().get_mut(&conn_id) {
            conn.is_open = false;
            if !conn.task.is_null() {
                unsafe {
                    let _: () = msg_send![
                        conn.task,
                        cancelWithCloseCode: 1000i64,
                        reason: std::ptr::null::<AnyObject>()
                    ];
                }
            }
        }
    });
}

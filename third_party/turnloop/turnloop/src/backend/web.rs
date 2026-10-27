//! Browser/Node host callbacks. The owner calls only turn(Now). See
//! docs/wasm.md for scheduling, cancellation, and the host allocation boundary.
//!
//! Socket options are `Unsupported` here, from the Backend trait's own defaults:
//! a browser host exposes `fetch` and `WebSocket`, not a socket, so there is no
//! `TCP_NODELAY`, keep-alive schedule, linger, buffer size or group membership to
//! set or read. `ListenOpts` never reaches this backend either, because listening
//! sockets are themselves unsupported (DESIGN §7.5).
use crate::slots::Slots;
use crate::{
    backend::{Backend, Budget, Event, Operation, Outcome, PollInfo, Request, Wake},
    *,
};
use js_sys::{Function, Uint8Array};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use wasm_bindgen::{JsCast, prelude::*};
#[wasm_bindgen(module = "/src/backend/web/host.js")]
extern "C" {
    #[wasm_bindgen(catch,js_name=createHost)]
    fn create_host(capacity: u32) -> std::result::Result<u32, JsValue>;
    fn now() -> f64;
    #[wasm_bindgen(catch)]
    fn configure(id: u32, schedule: &Function) -> std::result::Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn wake(id: u32) -> std::result::Result<(), JsValue>;
    #[wasm_bindgen(js_name=beginTurn)]
    fn begin_turn(id: u32);
    #[wasm_bindgen(catch,js_name=deadlineChanged)]
    fn deadline_changed(id: u32, deadline: f64) -> std::result::Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn open(id: u32, key: u64, kind: u32, url: &str) -> std::result::Result<(), JsValue>;
    #[wasm_bindgen(catch)]
    fn submit(
        id: u32,
        key: u64,
        handle: u64,
        kind: u32,
        bytes: &JsValue,
    ) -> std::result::Result<(), JsValue>;
    fn status(id: u32, key: u64) -> u32;
    fn value(id: u32, key: u64) -> JsValue;
    fn retire(id: u32, key: u64);
    fn cancel(id: u32, key: u64);
    fn release(id: u32, key: u64);
    fn dispose(id: u32);
    fn schedules(id: u32) -> u32;
    #[cfg(feature = "web-worker")]
    #[wasm_bindgen(js_name=workerSupported)]
    fn worker_supported() -> bool;
    #[cfg(feature = "web-worker")]
    #[wasm_bindgen(js_name=workerPending)]
    fn worker_pending(id: u32) -> bool;
    #[cfg(feature = "web-worker")]
    #[wasm_bindgen(catch, js_name=attachWorker)]
    fn attach_worker(
        id: u32,
        capacity: u32,
        accept: &Function,
    ) -> std::result::Result<JsValue, JsValue>;
    #[cfg(feature = "web-worker")]
    #[wasm_bindgen(catch, js_name=attachCondition)]
    fn attach_condition(
        id: u32,
        capacity: u32,
        accept: &Function,
    ) -> std::result::Result<JsValue, JsValue>;
}
fn error(_: JsValue) -> Error {
    Error::new(ErrorKind::Other)
}
/// Web resources cannot transfer through the native detached transport API.
#[derive(Debug)]
pub enum Detached {}
/// Host scheduler endpoint for an owning web instance.
pub struct WebWake {
    id: u32,
}
impl Wake for WebWake {
    fn wake(&self) -> Result<()> {
        wake(self.id).map_err(error)
    }
    fn syscall_count(&self) -> u64 {
        0
    }
}
struct Resource {
    handle: Handle,
    kind: u32,
}
struct Pending {
    request: Request,
    cancelled: bool,
}
/// Host callback driver for browser and Node agents.
pub struct Web {
    id: u32,
    wake: Arc<WebWake>,
    resources: Slots<Resource>,
    ops: Slots<Pending>,
    pool: BufferPool,
    failure: Option<Error>,
    #[cfg(feature = "web-worker")]
    worker: Option<Closure<dyn FnMut(u64, u64) -> bool>>,
    #[cfg(feature = "web-worker")]
    conditions: Vec<Closure<dyn FnMut(u64, u64) -> bool>>,
}
impl Web {
    pub(crate) fn configure(&mut self, schedule: &Function) -> Result<()> {
        configure(self.id, schedule).map_err(error)
    }
    /// Number of coalesced host turn requests, for contract instrumentation.
    pub fn schedule_count(&self) -> u32 {
        schedules(self.id)
    }
    #[cfg(feature = "web-worker")]
    pub(crate) fn worker_poster(&mut self, capacity: u32, poster: Poster) -> Result<JsValue> {
        if !worker_supported() {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if self.worker.is_some()
            || capacity == 0
            || !capacity.is_power_of_two()
            || capacity > 1_048_576
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let accept = Closure::wrap(Box::new(move |token, value| {
            poster.post(Token(token), Payload::U64(value)).is_ok()
        }) as Box<dyn FnMut(u64, u64) -> bool>);
        let descriptor =
            attach_worker(self.id, capacity, accept.as_ref().unchecked_ref()).map_err(error)?;
        self.worker = Some(accept);
        Ok(descriptor)
    }
    #[cfg(feature = "web-worker")]
    pub(crate) fn worker_condition(
        &mut self,
        condition: WaitCondition,
        capacity: u32,
    ) -> Result<JsValue> {
        if !worker_supported() {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if capacity == 0 || !capacity.is_power_of_two() || capacity > 1_048_576 {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        if self.conditions.len() == self.ops.len() {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        let accept = Closure::wrap(Box::new(move |kind: u64, value: u64| {
            if kind == 0 {
                condition.notify();
            } else {
                condition.store(value);
            }
            true
        }) as Box<dyn FnMut(u64, u64) -> bool>);
        let descriptor =
            attach_condition(self.id, capacity, accept.as_ref().unchecked_ref()).map_err(error)?;
        self.conditions.push(accept);
        Ok(descriptor)
    }
    fn worker_has_work(&self) -> bool {
        #[cfg(feature = "web-worker")]
        {
            worker_pending(self.id)
        }
        #[cfg(not(feature = "web-worker"))]
        {
            false
        }
    }
    fn resource(&self, h: Handle) -> Result<&Resource> {
        self.resources
            .get(h.index())
            .and_then(Option::as_ref)
            .filter(|r| r.handle == h)
            .ok_or(Error::new(ErrorKind::NotFound))
    }
}
// SAFETY: imports never retain Rust byte views. JS copies sends synchronously;
// receive data is copied only during poll into Request-owned memory. Cancellation
// invalidates JS operation generations before the terminal event frees buffers.
unsafe impl Backend for Web {
    type Wake = WebWake;
    type Detached = Detached;
    fn new(config: &Config, pool: BufferPool) -> Result<Self> {
        let id = create_host(
            config
                .max_operations
                .try_into()
                .map_err(|_| Error::new(ErrorKind::ResourceLimit))?,
        )
        .map_err(error)?;
        Ok(Self {
            id,
            wake: Arc::new(WebWake { id }),
            resources: Slots::new(config.max_handles),
            ops: Slots::new(config.max_operations),
            pool,
            failure: None,
            #[cfg(feature = "web-worker")]
            worker: None,
            #[cfg(feature = "web-worker")]
            conditions: Vec::new(),
        })
    }
    fn now(&self) -> Instant {
        Instant::from_duration(Duration::from_secs_f64(now() / 1000.0))
    }
    fn validate_timeout(&self, t: Timeout) -> Result<()> {
        if matches!(t, Timeout::Now) {
            Ok(())
        } else {
            Err(Error::new(ErrorKind::Unsupported))
        }
    }
    fn deadline_changed(&mut self, at: Option<Instant>) {
        if let Err(e) = deadline_changed(
            self.id,
            at.map_or(-1.0, |t| t.as_duration().as_secs_f64() * 1000.0),
        ) {
            self.failure = Some(error(e));
        }
    }
    fn waker(&self) -> Arc<WebWake> {
        self.wake.clone()
    }
    fn open(&mut self, h: Handle, spec: Open) -> Result<()> {
        let (kind, url) = match spec {
            Open::Fetch { url } => (1, url),
            Open::WebSocket { url } => (2, url),
            _ => return Err(Error::new(ErrorKind::Unsupported)),
        };
        if self.resources.get(h.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        open(self.id, h.key(), kind, &url).map_err(error)?;
        self.resources[h.index()] = Some(Resource { handle: h, kind });
        Ok(())
    }
    fn local_addr(&self, _: Handle) -> Result<SocketAddr> {
        Err(Error::new(ErrorKind::Unsupported))
    }
    fn submit(&mut self, request: Request) -> Result<()> {
        if matches!(
            request.operation,
            Operation::ProcessExit
                | Operation::WatchSignal
                | Operation::WatchFs
                | Operation::SendHandle(_)
                | Operation::RecvHandle
        ) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        let r = self.resource(request.handle)?;
        if self.ops.get(request.op.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let (kind, bytes) = match &request.operation {
            Operation::Connect if r.kind == 2 => (1, JsValue::UNDEFINED),
            Operation::Read {
                buf,
                multishot: false,
            } => {
                if matches!(buf,ReadBuf::Provided(b) if b.is_empty()) {
                    return Err(Error::new(ErrorKind::InvalidInput));
                }
                if self.ops.iter().flatten().any(|p| {
                    p.request.handle == request.handle
                        && matches!(p.request.operation, Operation::Read { .. })
                }) {
                    return Err(Error::new(ErrorKind::WouldBlock));
                }
                (2, JsValue::UNDEFINED)
            }
            Operation::Write(buf) if r.kind == 2 => {
                // Own a JS copy: WebSocket callbacks may outlive the Rust buffer.
                (3, Uint8Array::from(buf.as_slice()).into())
            }
            Operation::Shutdown if r.kind == 2 => (4, JsValue::UNDEFINED),
            _ => return Err(Error::new(ErrorKind::Unsupported)),
        };
        submit(
            self.id,
            request.op.key(),
            request.handle.key(),
            kind,
            &bytes,
        )
        .map_err(error)?;
        let i = request.op.index();
        self.ops[i] = Some(Pending {
            request,
            cancelled: false,
        });
        Ok(())
    }
    fn cancel(&mut self, op: OpId) -> Result<()> {
        let p = self
            .ops
            .get_mut(op.index())
            .and_then(Option::as_mut)
            .filter(|p| p.request.op == op)
            .ok_or(Error::new(ErrorKind::NotFound))?;
        if !p.cancelled {
            cancel(self.id, op.key());
            p.cancelled = true;
            wake(self.id).map_err(error)?;
        }
        Ok(())
    }
    fn has_work(&self) -> bool {
        self.failure.is_some()
            || self.worker_has_work()
            || self
                .ops
                .iter()
                .flatten()
                .any(|p| p.cancelled || status(self.id, p.request.op.key()) != 0)
    }
    fn poll(
        &mut self,
        timeout: Option<Duration>,
        // No listener exists on the web: nothing here creates a handle.
        _budget: Budget,
        events: &mut Vec<Event<Detached>>,
    ) -> Result<PollInfo> {
        if timeout != Some(Duration::ZERO) {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if let Some(e) = self.failure.take() {
            return Err(e);
        }
        begin_turn(self.id);
        for slot in &mut self.ops {
            if events.len() == events.capacity() {
                break;
            }
            let Some(p) = slot.as_mut() else {
                continue;
            };
            let op = p.request.op;
            let code = if p.cancelled {
                7
            } else {
                status(self.id, op.key())
            };
            let result = match code {
                0 => continue,
                1 => Ok(Outcome::Connected),
                3 => Ok(Outcome::Wrote(
                    value(self.id, op.key()).as_f64().expect("host byte count") as usize,
                )),
                4 => Ok(Outcome::Shutdown),
                5 => Err(Error::new(ErrorKind::Other)),
                6 => Ok(Outcome::Eof),
                7 => Ok(Outcome::Cancelled),
                2 => {
                    let bytes: Uint8Array = value(self.id, op.key()).unchecked_into();
                    let Operation::Read { buf, .. } = &p.request.operation else {
                        unreachable!()
                    };
                    let mut lease = None;
                    let output = match buf {
                        ReadBuf::Provided(b) => {
                            // SAFETY: the accepted Request exclusively owns this region until this terminal event.
                            unsafe { std::slice::from_raw_parts_mut(b.as_mut_ptr(), b.len()) }
                        }
                        ReadBuf::Pooled => {
                            let Some(b) = self.pool.acquire() else {
                                continue;
                            };
                            lease = Some(b);
                            lease.as_mut().expect("acquired").writable()
                        }
                    };
                    let n = bytes.length() as usize;
                    if n > output.len() {
                        Err(Error::new(ErrorKind::ResourceLimit))
                    } else {
                        bytes.copy_to(&mut output[..n]);
                        if let Some(b) = &mut lease {
                            b.set_len(n);
                        }
                        Ok(Outcome::Read { n, lease })
                    }
                }
                _ => unreachable!("host completion kind"),
            };
            retire(self.id, op.key());
            *slot = None;
            events.push(Event {
                op,
                terminal: true,
                result,
            });
        }
        if self.has_work() {
            wake(self.id).map_err(error)?;
        }
        // Draining callbacks performs neither a blocking wait nor native discovery.
        Ok(PollInfo::default())
    }
    fn release(&mut self, h: Handle) {
        if self.resource(h).is_ok() {
            release(self.id, h.key());
            self.resources[h.index()] = None;
        }
    }
    fn detach(&mut self, _: Handle) -> Result<Detached> {
        Err(Error::new(ErrorKind::Unsupported))
    }
    fn attach(&mut self, _: Handle, transport: Detached) -> Result<()> {
        match transport {}
    }
    fn integration(&mut self) -> Result<Integration> {
        Ok(Integration::HostCallback)
    }
}
impl Drop for Web {
    fn drop(&mut self) {
        dispose(self.id);
    }
}

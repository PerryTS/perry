//! Typed ownership around the C operation table. A store stays on its agent
//! thread; resource table entries carry a thread check as well because Wasmtime
//! requires its resource types to be Send.
use perry_abi::component_io as abi;
use std::{
    ptr,
    sync::{
        atomic::{AtomicPtr, Ordering},
        Arc,
    },
    thread::{self, ThreadId},
};
static RUNTIME: AtomicPtr<abi::Ops> = AtomicPtr::new(ptr::null_mut());

/// The link installer supplies an immutable, process-lifetime runtime table.
#[cfg(not(test))]
pub(crate) unsafe fn install(ops: *const abi::Ops) {
    // SAFETY: the installer supplies a live static table.
    let table = unsafe { &*ops };
    assert_eq!(table.abi_version, abi::ABI_VERSION);
    assert_eq!(table.size, std::mem::size_of::<abi::Ops>());
    RUNTIME.store(ops.cast_mut(), Ordering::Release);
}
#[derive(Debug, Clone, Copy)]
pub struct Error(pub abi::Error);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component I/O error {} (OS {})", self.0.kind, self.0.os)
    }
}
impl std::error::Error for Error {}
fn check(error: abi::Error) -> Result<(), Error> {
    if error.kind == 0 {
        Ok(())
    } else {
        Err(Error(error))
    }
}
#[derive(Clone, Copy)]
pub struct Client(&'static abi::Ops);
impl Client {
    pub fn current() -> Self {
        let ptr = RUNTIME.load(Ordering::Acquire);
        Self(if ptr.is_null() {
            perry_component_io::standalone::ops()
        } else {
            // SAFETY: install accepts only immutable process-lifetime tables.
            unsafe { &*ptr }
        })
    }
    pub fn timer(self, duration: u64) -> wasmtime::Result<Arc<Operation>> {
        Ok(Arc::new(self.submit(
            None,
            abi::TIMER,
            &[],
            0,
            None,
            duration,
        )?))
    }
    pub fn resolve(self, name: &str) -> Result<Operation, Error> {
        self.submit(None, abi::RESOLVE, name.as_bytes(), name.len(), None, 0)
    }
    pub fn submit(
        self,
        socket: Option<Arc<Socket>>,
        kind: u32,
        bytes: &[u8],
        len: usize,
        address: Option<abi::Address>,
        delay: u64,
    ) -> Result<Operation, Error> {
        if let Some(s) = &socket {
            s.check_thread();
            assert!(ptr::eq(s.table, self.0));
        }
        let resource = socket
            .as_ref()
            .map_or(ptr::null_mut(), |s| s.raw as abi::Opaque);
        let addr = address.as_ref().map_or(ptr::null(), ptr::from_ref);
        let mut raw = ptr::null_mut();
        // SAFETY: the socket stays owned by the operation; input is copied by
        // submit, or len asks it to allocate its own native read buffer.
        check(unsafe {
            (self.0.submit)(resource, kind, bytes.as_ptr(), len, addr, delay, &mut raw)
        })?;
        Ok(Operation {
            raw: raw as usize,
            table: self.0,
            thread: thread::current().id(),
            _socket: socket,
        })
    }
    /// Transfers ownership of an owned native socket even if adoption fails.
    pub unsafe fn adopt(
        self,
        raw: usize,
        listener: bool,
        destination: Option<abi::Address>,
    ) -> Result<(Arc<Socket>, Option<Operation>), Error> {
        let addr = destination.as_ref().map_or(ptr::null(), ptr::from_ref);
        let (mut resource, mut operation) = (ptr::null_mut(), ptr::null_mut());
        // SAFETY: caller transferred the native socket, outputs are writable.
        check(unsafe { (self.0.adopt)(raw, listener, addr, &mut resource, &mut operation) })?;
        let socket = Arc::new(Socket {
            raw: resource as usize,
            table: self.0,
            thread: thread::current().id(),
        });
        let operation = (!operation.is_null()).then(|| Operation {
            raw: operation as usize,
            table: self.0,
            thread: thread::current().id(),
            _socket: Some(socket.clone()),
        });
        Ok((socket, operation))
    }
    pub fn poll(self, operations: &[&Operation], block: bool) -> wasmtime::Result<()> {
        if operations.is_empty() {
            return Ok(());
        }
        let pointers = operations
            .iter()
            .map(|op| {
                op.check_thread();
                wasmtime::ensure!(
                    ptr::eq(op.table, self.0),
                    "different embedding drivers in one poll"
                );
                Ok(op.raw as abi::Opaque)
            })
            .collect::<wasmtime::Result<Vec<_>>>()?;
        // SAFETY: borrowed operations and their storage live throughout this turn.
        check(unsafe { (self.0.poll)(pointers.as_ptr(), pointers.len(), block) })?;
        Ok(())
    }
}
pub struct Socket {
    raw: usize,
    table: &'static abi::Ops,
    thread: ThreadId,
}
impl Socket {
    fn check_thread(&self) {
        assert_eq!(
            self.thread,
            thread::current().id(),
            "native socket belongs to its embedding thread"
        );
    }
    pub fn raw(&self) -> Result<usize, Error> {
        self.check_thread();
        let mut value = 0;
        // SAFETY: this borrows a live resource and a writable scalar.
        check(unsafe { (self.table.raw_socket)(self.raw as abi::Opaque, &mut value) })?;
        Ok(value)
    }
    pub fn release(self) -> Result<usize, Error> {
        self.check_thread();
        let mut raw = 0;
        // SAFETY: consuming self requires the final socket owner, and detach
        // additionally checks the driver's operation/acknowledgement state.
        check(unsafe { (self.table.release_socket)(self.raw as abi::Opaque, &mut raw) })?;
        std::mem::forget(self);
        Ok(raw)
    }
    pub fn option(&self, tag: u32, set: bool, mut values: [u64; 4]) -> Result<[u64; 4], Error> {
        self.check_thread();
        // SAFETY: the table borrows the socket and four-word option buffer.
        check(unsafe { (self.table.option)(self.raw as abi::Opaque, tag, set, &mut values) })?;
        Ok(values)
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.check_thread();
        // SAFETY: operations retain an Arc, so this final owner has no native
        // operation outstanding. Close waits for its exact acknowledgement.
        unsafe {
            (self.table.resource_drop)(self.raw as abi::Opaque);
        }
    }
}
pub struct Operation {
    raw: usize,
    table: &'static abi::Ops,
    thread: ThreadId,
    _socket: Option<Arc<Socket>>,
}
pub struct Completion {
    pub kind: u32,
    pub error: abi::Error,
    pub address: abi::Address,
    pub bytes: Vec<u8>,
    pub addresses: Vec<abi::Address>,
    pub socket: Option<Arc<Socket>>,
}
impl Operation {
    fn check_thread(&self) {
        assert_eq!(
            self.thread,
            thread::current().id(),
            "native I/O belongs to its embedding thread"
        );
    }
    pub fn ready(&self) -> bool {
        self.check_thread();
        // SAFETY: this object owns an unconsumed native operation on this thread.
        unsafe { (self.table.ready)(self.raw as abi::Opaque) }
    }
    pub fn finish(self) -> Result<Completion, Error> {
        self.check_thread();
        let mut out = abi::Completion::default();
        // SAFETY: take consumes the operation only when returning success.
        check(unsafe { (self.table.take)(self.raw as abi::Opaque, &mut out) })?;
        let this = std::mem::ManuallyDrop::new(self);
        let socket = (!out.resource.is_null()).then(|| {
            Arc::new(Socket {
                raw: out.resource as usize,
                table: this.table,
                thread: this.thread,
            })
        });
        out.resource = ptr::null_mut();
        // SAFETY: the returned buffers remain live until completion_drop below.
        let bytes = if out.bytes.len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(out.bytes.data, out.bytes.len).to_vec() }
        };
        // SAFETY: the producing allocator owns exactly addresses.len entries.
        let addresses = if out.addresses.len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(out.addresses.data, out.addresses.len).to_vec() }
        };
        let complete = Completion {
            kind: out.kind,
            error: out.error,
            address: out.address,
            bytes,
            addresses,
            socket,
        };
        // SAFETY: buffers go back to their allocator; socket ownership moved above.
        unsafe {
            (this.table.completion_drop)(out);
        }
        // SAFETY: take consumed the opaque operation. Release only its retained
        // socket Arc, not Operation::drop (which would destroy it a second time).
        unsafe {
            drop(ptr::read(&this._socket));
        }
        Ok(complete)
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.check_thread();
        // SAFETY: the final owner transfers the operation for cancel/drain/free.
        unsafe {
            (self.table.operation_drop)(self.raw as abi::Opaque);
        }
    }
}

use super::*;

#[test]
fn direct_discovery_and_event_queue_draining_have_distinct_counts() {
    use windows_sys::Win32::{Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject};
    let mut backend = Iocp::new(&Config::default(), BufferPool::new(2, 64)).expect("backend");
    let mut events = Vec::with_capacity(1);
    for wake in [false, true] {
        if wake {
            backend.port.post(WAKE, 0).expect("wake");
        }
        let info = backend
            .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
            .expect("discovery");
        assert_eq!(
            (info.waits, info.discovery_polls, info.zero_event_waits),
            (0, 1, u32::from(!wake))
        );
        assert!(events.is_empty());
    }
    let Integration::Event(event) = backend.integration().expect("event helper") else {
        panic!("Event required");
    };
    backend.port.post(WAKE, 0).expect("helper packet");
    // SAFETY: backend owns the event throughout this bounded external wait.
    let result = unsafe { WaitForSingleObject(event as _, 2000) };
    assert_eq!(result, WAIT_OBJECT_0, "helper must forward a real packet");
    let info = backend
        .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut events)
        .expect("helper drain");
    assert_eq!(
        (info.waits, info.discovery_polls, info.zero_event_waits),
        (0, 0, 0)
    );
    assert!(events.is_empty());
}

// These operations model already-dequeued kernel completions. No native I/O
// is submitted, so unwind must discard their metadata rather than wait on it.
pub(super) struct Synthetic {
    pub backend: Iocp,
    pub entries: [Entry; 2],
    pub ops: [OpId; 2],
}
impl Synthetic {
    pub fn new() -> Self {
        let config = Config {
            max_handles: 2,
            max_operations: 2,
            ..Config::default()
        };
        let mut backend = Iocp::new(&config, BufferPool::new(2, 64)).expect("backend");
        let handle = Handle { owner: 1, key: 0 };
        backend
            .open(
                handle,
                Open::Tcp {
                    addr: ([127, 0, 0, 1], 9).into(),
                    opts: TcpOpts::default(),
                },
            )
            .expect("socket resource");
        let ops = [OpId { owner: 1, key: 0 }, OpId { owner: 1, key: 1 }];
        let entries = std::array::from_fn(|i| {
            backend
                .submit(Request {
                    op: ops[i],
                    handle,
                    operation: Operation::Write(WriteBuf::Owned(vec![0; i + 1])),
                })
                .expect("synthetic request");
            let pending = backend.ops[i].as_mut().expect("op");
            pending.waiting = true;
            pending.queued = false;
            pending.stage = Stage::Io;
            Entry {
                key: 1,
                overlapped: backend.kernel_ptr(i) as usize,
                bytes: (i + 1) as u32,
                status: 0,
            }
        });
        backend.ready.clear();
        Self {
            backend,
            entries,
            ops,
        }
    }
    pub fn assert_delivered(&mut self) {
        let mut out = Vec::with_capacity(1);
        for (i, op) in self.ops.into_iter().enumerate() {
            let info = self
                .backend
                .poll(Some(Duration::ZERO), Budget::UNLIMITED, &mut out)
                .expect("drain valid packet");
            assert_eq!((info.waits, info.discovery_polls), (0, 0));
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].op, op);
            assert!(out[0].terminal);
            assert!(matches!(out[0].result, Ok(Outcome::Wrote(n)) if n == i + 1));
            out.clear();
        }
        assert!(self.backend.ops.iter().all(Option::is_none));
        assert!(!self.backend.has_work());
    }
}
impl Drop for Synthetic {
    fn drop(&mut self) {
        self.backend.ops.iter_mut().for_each(|op| *op = None);
        self.backend.ready.clear();
    }
}
#[test]
fn invalid_entry_does_not_discard_later_completions() {
    let mut fixture = Synthetic::new();
    let batch = [fixture.entries[0], Entry::default(), fixture.entries[1]];
    assert_eq!(
        fixture
            .backend
            .entries(&batch)
            .expect_err("invalid packet")
            .kind,
        ErrorKind::InvalidInput
    );
    fixture.assert_delivered();
}

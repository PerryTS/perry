use super::*;
use crate::{AsyncImport, Host, Options};
use futures_lite::future::{block_on, poll_once};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

struct Cleanup {
    registry: Registry,
    origin: InvocationId,
    dropped: Arc<AtomicUsize>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        assert_eq!(current_invocation(), Some(self.origin));
        assert!(
            self.registry.snapshot().contains(&self.origin),
            "captured context was released before its future cleanup"
        );
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
type Deferred = Arc<Mutex<Vec<Pin<Box<dyn Future<Output = ()> + Send>>>>>;

#[test]
fn deferred_callback_context_survives_guest_completion_and_retirement_until_cleanup() {
    for (poll_cleanup, retire) in [(false, false), (true, false), (false, true), (true, true)] {
        let registry = Registry::default();
        let captured_registry = registry.clone();
        let deferred: Deferred = Arc::default();
        let scheduled = deferred.clone();
        let drops = Arc::new(AtomicUsize::new(0));
        let dropped = drops.clone();
        let callback: AsyncImport = Arc::new(move |_| {
            let origin = current_invocation().unwrap();
            let (id, lease) = captured_registry
                .capture(&[], Some(origin))
                .expect("registered guest origin");
            let capture = Captured {
                id: Some(id),
                _lease: Some(lease),
            };
            let cleanup = Cleanup {
                registry: captured_registry.clone(),
                origin,
                dropped: dropped.clone(),
            };
            scheduled
                .lock()
                .unwrap()
                .push(Box::pin(captured(capture, async move {
                    let _cleanup = cleanup;
                    assert_eq!(current_invocation(), Some(origin));
                    std::future::pending::<()>().await;
                })));
            Box::pin(async { Ok(None) })
        });
        let world=perry_wit::parse("package test:driver; interface clock { wait-for: async func(value: u64); } world plugin { import clock; export run: async func(value:u64) -> u64; }",None).unwrap();
        let heap =
            include_str!("../async_values/heap.wat").replacen("(module", "(core module $heap", 1);
        let component = include_str!("../async_values/task_context.component.wat")
            .replace(";; HEAP", &heap)
            .replace("\"clock\"", "\"test:driver/clock\"");
        let imports = std::collections::BTreeMap::from([(
            "test:driver/clock".into(),
            std::collections::BTreeMap::from([("wait-for".into(), callback)]),
        )]);
        let mut host = Host::new_with_async_imports(
            &wat::parse_str(component).unwrap(),
            &world,
            &Default::default(),
            &imports,
            &Options::default(),
            Arc::new(|_| Ok(Vec::new())),
        )
        .unwrap();
        host.live
            .as_mut()
            .unwrap()
            .store
            .data_mut()
            .origins
            .registry = registry.clone();
        let (mut driver, client) = host.into_driver().unwrap();
        let mut call = client.call("", "run", &[json!({"$bigint":"7"})]).unwrap();
        let origin = call.id();
        let mut result = None;
        for _ in 0..128 {
            assert!(block_on(poll_once(&mut driver)).is_none());
            if let Some(reply) = block_on(poll_once(&mut call)) {
                result = Some(reply.unwrap());
                break;
            }
        }
        assert_eq!(result, Some(Some(json!({"$bigint":"7"}))));
        for _ in 0..128 {
            assert!(block_on(poll_once(&mut driver)).is_none());
            if !driver.is_active() {
                break;
            }
        }
        assert!(
            !driver.is_active(),
            "the actual guest must have finished before deferred cleanup"
        );
        if retire {
            drop(driver);
            assert!(client.call("", "run", &[]).is_err());
        }
        assert_eq!(client.retained_origins(), vec![origin]);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let mut cleanup = deferred.lock().unwrap().pop().unwrap();
        if poll_cleanup {
            assert!(block_on(poll_once(cleanup.as_mut())).is_none());
            assert_eq!(current_invocation(), None);
        }
        drop(cleanup);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(client.retained_origins().is_empty());
        assert_eq!(current_invocation(), None);
    }
}

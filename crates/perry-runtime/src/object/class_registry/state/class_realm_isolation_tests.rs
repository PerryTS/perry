use super::*;
use std::sync::{Arc, Barrier, Mutex};

/// #8001 — a declared class's lazily materialized `.prototype` belongs to
/// the agent that materialized it, not to whichever thread first used the
/// process-wide class id.
///
/// The class id and name are codegen facts shared by both agents. The heap
/// object stored under that id is not: each thread owns a separate realm and
/// arena. Bootstraps are serialized because the test harness's globalThis
/// root slot is process-global, while the barrier keeps both arenas live
/// until both addresses have been observed. Before #8001, agent B returned
/// agent A's cached pointer at the first-thread-wins gate.
#[test]
fn a_second_agents_declared_prototype_address_is_its_own() {
    const CLASS_ID: u32 = 0x7d01_8001;
    const CLASS_NAME: &[u8] = b"Issue8001Class";

    unsafe {
        js_register_class_name(CLASS_ID, CLASS_NAME.as_ptr(), CLASS_NAME.len() as u32);
    }

    let bootstrap_gate = Arc::new(Mutex::new(()));
    let both_alive = Arc::new(Barrier::new(2));
    let agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
        move || -> usize {
            let addr = {
                let _serialized = gate.lock().expect("bootstrap gate");
                let bits = class_decl_prototype_value(CLASS_ID).to_bits();
                assert_eq!(
                    bits & crate::value::TAG_MASK,
                    crate::value::POINTER_TAG,
                    "the class prototype must materialize before isolation is tested"
                );
                (bits & crate::value::POINTER_MASK) as usize
            };
            barrier.wait();
            addr
        }
    };
    let spawn_agent = |gate: Arc<Mutex<()>>, barrier: Arc<Barrier>| {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(agent(gate, barrier))
            .expect("spawn realm agent")
    };

    let a = spawn_agent(Arc::clone(&bootstrap_gate), Arc::clone(&both_alive));
    let b = spawn_agent(bootstrap_gate, both_alive);
    let a_addr = a.join().expect("agent A panicked");
    let b_addr = b.join().expect("agent B panicked");

    for (label, addr) in [("agent A", a_addr), ("agent B", b_addr)] {
        assert_ne!(
            addr, 0,
            "{label} did not materialize a real class prototype; distinctness would be vacuous"
        );
    }
    assert_ne!(
        a_addr, b_addr,
        "two live agents must materialize their own Class.prototype objects (#8001)"
    );
}

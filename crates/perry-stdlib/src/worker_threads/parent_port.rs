//! A Worker's transferable data endpoint, separate from its lifecycle commands.
use super::*;
thread_local! {
    static CURRENT: Cell<(u64, Lease)> = Cell::new((0, Lease::default()));
}
pub(super) fn id() -> u64 {
    CURRENT.with(|slot| slot.get().0)
}
pub(super) fn install(ticket: &port_transport::Ticket) {
    let lease = port_transport::adopt(ticket).expect("fresh worker data endpoint");
    CURRENT.with(|slot| slot.set((0, lease)));
    worker_parent_port_object();
}
pub(super) fn worker_parent_port_object() -> *mut perry_runtime::object::ObjectHeader {
    let (old_id, mut lease) = CURRENT.with(Cell::get);
    if let Some(bits) =
        MESSAGE_PORTS.with(|ports| ports.borrow().get(&old_id).map(|p| p.object_bits))
    {
        return perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut _;
    }
    ensure_environment_data_gc_scanner();
    message_port::register_codec();
    if lease == Lease::default() {
        lease = port_transport::pair().0;
    }
    let id = next_port_id();
    CURRENT.with(|slot| slot.set((id, lease)));
    MESSAGE_PORTS.with(|ports| {
        ports.borrow_mut().insert(
            id,
            MessagePortState {
                lease,
                ..Default::default()
            },
        );
    });
    message_port::message_port_object(id)
}
pub(super) fn is_closed() -> bool {
    MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&id())
            .is_some_and(MessagePortState::closed)
    })
}

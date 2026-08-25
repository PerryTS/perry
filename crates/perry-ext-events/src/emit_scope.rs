use super::*;

pub(super) struct EventEmitterEmitCall {
    pub(super) handle: Handle,
    pub(super) event_value: TransientRootedNanbox,
    pub(super) args_ptr: TransientRootedAddr,
}

pub(super) unsafe extern "C" fn event_emitter_emit_thunk(data: *mut c_void) -> f64 {
    let call = &mut *(data as *mut EventEmitterEmitCall);
    let Some(event_name) = event_name_from_bits(call.event_value.get().to_bits() as i64) else {
        return f64::from_bits(0x7FFC_0000_0000_0003);
    };
    js_event_emitter_emit_impl(
        call.handle,
        &event_name,
        call.args_ptr.get() as *mut ArrayHeader,
    )
}

pub(super) struct EventEmitterEmit0Call {
    pub(super) handle: Handle,
    pub(super) event_value: TransientRootedNanbox,
}

pub(super) unsafe extern "C" fn event_emitter_emit0_thunk(data: *mut c_void) -> f64 {
    let call = &mut *(data as *mut EventEmitterEmit0Call);
    let Some(event_name) = event_name_from_bits(call.event_value.get().to_bits() as i64) else {
        return f64::from_bits(0x7FFC_0000_0000_0003);
    };
    js_event_emitter_emit0_impl(call.handle, &event_name)
}

//! Worker metadata lives on the exported prototype; its private ID survives exit.
use super::*;
use crate::feature_hooks::Hook;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum WorkerMetadataField {
    ThreadId,
    ThreadName,
    ResourceLimits,
}

type MetadataProvider = extern "C" fn(u64, WorkerMetadataField) -> f64;
static PROVIDER: Hook<MetadataProvider> = Hook::empty();
static WORKER_ID: object::IntrinsicPrivateReadSite = object::IntrinsicPrivateReadSite::new(
    &object::runtime_layouts::intrinsic_private_atoms::WORKER_ID,
);

pub fn register_worker_metadata_provider(provider: MetadataProvider) {
    PROVIDER.set(provider);
}

pub fn worker_id(receiver: f64) -> Option<u64> {
    if !JSValue::from_bits(receiver.to_bits()).is_pointer() {
        return None;
    }
    let address = crate::value::js_nanbox_get_pointer(receiver) as usize;
    match unsafe { crate::value::addr_class::try_read_gc_header(address) } {
        Some(header) if crate::gc_type_is_object!(header.obj_type) => {}
        _ => return None,
    }
    WORKER_ID.read(receiver).map(f64::to_bits)
}

fn metadata_receiver_id(receiver: f64) -> Option<u64> {
    if let Some(id) = worker_id(receiver) {
        return Some(id);
    }
    // Unlike intrinsic private brands, Node Worker metadata also accepts
    // derived objects and transparent Proxies. Keep this walk operation-local.
    let scope = crate::gc::RuntimeHandleScope::new();
    let current = scope.root_nanbox_f64(receiver);
    let tortoise = scope.root_nanbox_f64(receiver);
    let mut power = 1usize;
    let mut lap = 0usize;
    // Match the rooted Brent walk used by OrdinaryHasInstance: both moving
    // identities stay rooted across prototype materialization or Proxy checks.
    for _ in 0..100_000 {
        let value = current.get_nanbox_f64();
        if let Some(id) = worker_id(value) {
            return Some(id);
        }
        let next = if let Some(target) = crate::proxy_hooks::proxy_checked_target(value) {
            target
        } else {
            if !JSValue::from_bits(value.to_bits()).is_pointer() {
                return None;
            }
            object::js_object_get_prototype_of(value)
        };
        current.set_nanbox_f64(next);
        if current.get_nanbox_u64() == tortoise.get_nanbox_u64() {
            return None;
        }
        lap += 1;
        if lap == power {
            tortoise.set_nanbox_f64(current.get_nanbox_f64());
            power <<= 1;
            lap = 0;
        }
    }
    None
}

pub fn worker_object_with_id(id: u64) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(object::native_module::worker_constructor_value());
    let prototype = scope.root_nanbox_f64(object::js_function_prototype_value_for_read(
        constructor.get_nanbox_f64(),
    ));
    let object = scope.root_raw_mut_ptr(object::js_object_alloc(0, 0));
    object::intrinsic_private_add(
        boxed_object(object.get_raw_mut_ptr()),
        "[[WorkerId]]",
        f64::from_bits(id),
    );
    object::js_object_set_prototype_of(
        boxed_object(object.get_raw_mut_ptr()),
        prototype.get_nanbox_f64(),
    );
    object.get_raw_mut_ptr()
}

extern "C" fn metadata_getter(closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    let field = crate::closure::js_closure_get_capture_f64(closure, 0) as u32;
    let id = metadata_receiver_id(this.as_f64());
    if let (Some(id), Some(provider)) = (id, PROVIDER.peek()) {
        let field = match field {
            0 => WorkerMetadataField::ThreadId,
            1 => WorkerMetadataField::ThreadName,
            _ => WorkerMetadataField::ResourceLimits,
        };
        return provider(id, field);
    }
    crate::exception::throw_core(crate::error::CoreError::type_error(key("Invalid Worker receiver")))
}

pub(crate) fn install_worker_metadata_getters(prototype: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_nanbox_f64(prototype);
    for (index, name) in ["threadId", "threadName", "resourceLimits"]
        .into_iter()
        .enumerate()
    {
        let capture = [(index as f64).to_bits()];
        let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc_init(
            crate::fn_info!(metadata_getter, 0; with_declared(0), with_flags(crate::closure::FN_NON_CONSTRUCTOR)),
            1,
            capture.as_ptr(),
        ));
        object::set_bound_native_closure_name(getter.get_raw_mut_ptr(), &format!("get {name}"));
        object::install_fresh_accessor_property(
            crate::value::js_nanbox_get_pointer(prototype.get_nanbox_f64()) as usize,
            name.to_string(),
            object::AccessorDescriptor {
                get: crate::value::js_nanbox_pointer(
                    getter.get_raw_mut_ptr::<ClosureHeader>() as i64
                )
                .to_bits(),
                set: 0,
            },
            object::PropertyAttrs::new(false, false, true),
        );
    }
}

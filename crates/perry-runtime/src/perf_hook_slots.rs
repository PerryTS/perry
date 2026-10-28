//! Installer-selected performance timeline and native-object operations.
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
static HISTOGRAM_METHOD: Hook<unsafe fn(*const ObjectHeader, &str, &[f64]) -> Option<f64>> =
    Hook::empty();
pub(crate) unsafe fn histogram_method(
    obj: *const ObjectHeader,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    HISTOGRAM_METHOD
        .get()
        .map_or_else(|| None, |run| unsafe { run(obj, method, args) })
}
static HISTOGRAM_PROPERTY: Hook<unsafe fn(f64, &str) -> Option<f64>> = Hook::empty();
pub(crate) unsafe fn histogram_property(namespace_obj: f64, property: &str) -> Option<f64> {
    HISTOGRAM_PROPERTY
        .get()
        .map_or_else(|| None, |run| unsafe { run(namespace_obj, property) })
}
static ATTACH_PERF_HOOKS_CONSTRUCTOR: Hook<unsafe fn(&str, f64, usize) -> ()> = Hook::empty();
pub(crate) unsafe fn attach_perf_hooks_constructor(
    class_name: &str,
    constructor_value: f64,
    closure_addr: usize,
) -> () {
    ATTACH_PERF_HOOKS_CONSTRUCTOR.get().map_or_else(
        || (),
        |run| unsafe { run(class_name, constructor_value, closure_addr) },
    )
}
static CONSTRUCT_PERF_HOOKS_CLASS: Hook<unsafe fn(&str, *const f64, usize) -> Option<f64>> =
    Hook::empty();
pub(crate) unsafe fn construct_perf_hooks_class(
    class_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    CONSTRUCT_PERF_HOOKS_CLASS.get().map_or_else(
        || None,
        |run| unsafe { run(class_name, args_ptr, args_len) },
    )
}
static CURRENT_LIST_GET_BY_NAME: Hook<unsafe fn(f64) -> f64> = Hook::empty();
pub(crate) unsafe fn current_list_get_by_name(name_val: f64) -> f64 {
    CURRENT_LIST_GET_BY_NAME.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run(name_val) },
    )
}
static CURRENT_LIST_GET_BY_TYPE: Hook<unsafe fn(f64) -> f64> = Hook::empty();
pub(crate) unsafe fn current_list_get_by_type(type_val: f64) -> f64 {
    CURRENT_LIST_GET_BY_TYPE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run(type_val) },
    )
}
static CURRENT_LIST_GET_ENTRIES: Hook<unsafe fn() -> f64> = Hook::empty();
pub(crate) unsafe fn current_list_get_entries() -> f64 {
    CURRENT_LIST_GET_ENTRIES.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run() },
    )
}
static IS_NODE_TIMING_OBJECT: Hook<unsafe fn(*const crate::object::ObjectHeader) -> bool> =
    Hook::empty();
pub(crate) unsafe fn is_node_timing_object(obj: *const crate::object::ObjectHeader) -> bool {
    IS_NODE_TIMING_OBJECT
        .get()
        .map_or_else(|| false, |run| unsafe { run(obj) })
}
static IS_PERF_ENTRY_OBJECT: Hook<unsafe fn(*const crate::object::ObjectHeader) -> bool> =
    Hook::empty();
pub(crate) unsafe fn is_perf_entry_object(obj: *const crate::object::ObjectHeader) -> bool {
    IS_PERF_ENTRY_OBJECT
        .get()
        .map_or_else(|| false, |run| unsafe { run(obj) })
}
static IS_PERF_ENTRY_OBJECT_INSTANCE_OF: Hook<
    unsafe fn(*const crate::object::ObjectHeader, u32) -> Option<bool>,
> = Hook::empty();
pub(crate) unsafe fn is_perf_entry_object_instance_of(
    obj: *const crate::object::ObjectHeader,
    class_id: u32,
) -> Option<bool> {
    IS_PERF_ENTRY_OBJECT_INSTANCE_OF
        .get()
        .map_or_else(|| None, |run| unsafe { run(obj, class_id) })
}
static IS_PERF_HOOKS_SHAPE_INSTANCE_OF: Hook<fn(f64, u32) -> Option<bool>> = Hook::empty();
pub(crate) fn is_perf_hooks_shape_instance_of(value: f64, class_id: u32) -> Option<bool> {
    IS_PERF_HOOKS_SHAPE_INSTANCE_OF
        .get()
        .map_or_else(|| None, |run| run(value, class_id))
}
static IS_PERF_OBSERVER_LIST_VALUE: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_perf_observer_list_value(value: f64) -> bool {
    IS_PERF_OBSERVER_LIST_VALUE
        .get()
        .map_or_else(|| false, |run| run(value))
}
static IS_PERFORMANCE_NAMESPACE_VALUE: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_performance_namespace_value(value: f64) -> bool {
    IS_PERFORMANCE_NAMESPACE_VALUE
        .get()
        .map_or_else(|| false, |run| run(value))
}
static JS_PERF_CLEAR_MARKS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_clear_marks(name_val: f64) -> f64 {
    JS_PERF_CLEAR_MARKS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name_val),
    )
}
static JS_PERF_CLEAR_MEASURES: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_clear_measures(name_val: f64) -> f64 {
    JS_PERF_CLEAR_MEASURES.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name_val),
    )
}
static JS_PERF_CLEAR_RESOURCE_TIMINGS: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_perf_clear_resource_timings() -> f64 {
    JS_PERF_CLEAR_RESOURCE_TIMINGS
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PERF_EVENT_LOOP_UTILIZATION: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_event_loop_utilization(util1: f64, util2: f64) -> f64 {
    JS_PERF_EVENT_LOOP_UTILIZATION.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(util1, util2),
    )
}
static JS_PERF_GET_ENTRIES: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_perf_get_entries() -> f64 {
    JS_PERF_GET_ENTRIES
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PERF_GET_ENTRIES_BY_NAME: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_get_entries_by_name(name_val: f64, type_val: f64) -> f64 {
    JS_PERF_GET_ENTRIES_BY_NAME.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name_val, type_val),
    )
}
static JS_PERF_GET_ENTRIES_BY_TYPE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_get_entries_by_type(type_val: f64) -> f64 {
    JS_PERF_GET_ENTRIES_BY_TYPE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(type_val),
    )
}
static JS_PERF_ILLEGAL_CONSTRUCTOR: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_perf_illegal_constructor() -> f64 {
    JS_PERF_ILLEGAL_CONSTRUCTOR
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PERF_MARK: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_mark(name_val: f64, options_val: f64) -> f64 {
    JS_PERF_MARK.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name_val, options_val),
    )
}
static JS_PERF_MARK_RESOURCE_TIMING: Hook<
    extern "C" fn(f64, f64, f64, f64, f64, f64, f64, f64) -> f64,
> = Hook::empty();
pub(crate) fn js_perf_mark_resource_timing(
    timing_info: f64,
    requested_url: f64,
    initiator_type: f64,
    _global: f64,
    cache_mode: f64,
    _body_info: f64,
    response_status: f64,
    delivery_type: f64,
) -> f64 {
    JS_PERF_MARK_RESOURCE_TIMING.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| {
            run(
                timing_info,
                requested_url,
                initiator_type,
                _global,
                cache_mode,
                _body_info,
                response_status,
                delivery_type,
            )
        },
    )
}
static JS_PERF_MEASURE: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_measure(name_val: f64, arg2: f64, arg3: f64) -> f64 {
    JS_PERF_MEASURE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name_val, arg2, arg3),
    )
}
static JS_PERF_NODE_TIMING: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_perf_node_timing() -> f64 {
    JS_PERF_NODE_TIMING
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_PERF_OBSERVER_DISCONNECT: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_observer_disconnect(obs_val: f64) -> f64 {
    JS_PERF_OBSERVER_DISCONNECT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(obs_val),
    )
}
static JS_PERF_OBSERVER_OBSERVE: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_observer_observe(obs_val: f64, opts: f64) -> f64 {
    JS_PERF_OBSERVER_OBSERVE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(obs_val, opts),
    )
}
static JS_PERF_OBSERVER_TAKE_RECORDS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_observer_take_records(obs_val: f64) -> f64 {
    JS_PERF_OBSERVER_TAKE_RECORDS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(obs_val),
    )
}
static JS_PERF_SET_RESOURCE_TIMING_BUFFER_SIZE: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_set_resource_timing_buffer_size(n: f64) -> f64 {
    JS_PERF_SET_RESOURCE_TIMING_BUFFER_SIZE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run(n))
}
static JS_PERF_TIMERIFY: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
pub(crate) fn js_perf_timerify(fn_value: f64, options: f64) -> f64 {
    JS_PERF_TIMERIFY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(fn_value, options),
    )
}
static JS_PERF_TO_JSON: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) fn js_perf_to_json() -> f64 {
    JS_PERF_TO_JSON
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static NODE_TIMING_TO_JSON: Hook<unsafe fn(f64) -> f64> = Hook::empty();
pub(crate) unsafe fn node_timing_to_json(this: f64) -> f64 {
    NODE_TIMING_TO_JSON.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run(this) },
    )
}
static NOTE_EVENT_LOOP_START: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn note_event_loop_start() -> () {
    NOTE_EVENT_LOOP_START.get().map_or_else(|| (), |run| run())
}
static PERF_ENTRY_TO_JSON: Hook<unsafe fn(f64) -> f64> = Hook::empty();
pub(crate) unsafe fn perf_entry_to_json(this: f64) -> f64 {
    PERF_ENTRY_TO_JSON.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run(this) },
    )
}
static PERF_SUPPORTED_ENTRY_TYPES_VALUE: Hook<fn() -> f64> = Hook::empty();
pub(crate) fn perf_supported_entry_types_value() -> f64 {
    PERF_SUPPORTED_ENTRY_TYPES_VALUE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static PERFORMANCE_NAMESPACE: Hook<fn() -> f64> = Hook::empty();
pub(crate) fn performance_namespace() -> f64 {
    PERFORMANCE_NAMESPACE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static PERFORMANCE_PROTOTYPE_METHOD_VALUE: Hook<fn(&str) -> Option<f64>> = Hook::empty();
pub(crate) fn performance_prototype_method_value(name: &str) -> Option<f64> {
    PERFORMANCE_PROTOTYPE_METHOD_VALUE
        .get()
        .map_or_else(|| None, |run| run(name))
}
static VALIDATE_PERF_LIST_FILTER_ARG: Hook<fn(f64, &str, bool) -> ()> = Hook::empty();
pub(crate) fn validate_perf_list_filter_arg(value: f64, name: &str, missing: bool) -> () {
    VALIDATE_PERF_LIST_FILTER_ARG
        .get()
        .map_or_else(|| (), |run| run(value, name, missing))
}
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register();
    }
}
fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::perf_hooks::scan_perf_entries_roots_mut",
        crate::perf_hooks::scan_perf_entries_roots_mut,
    );
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::perf_histogram::scan_histogram_roots_mut",
        crate::perf_histogram::scan_histogram_roots_mut,
    );
}

pub(crate) fn install() {
    crate::perry_install_once!({
        crate::perf_histogram::install_tls_resolver();
        HISTOGRAM_METHOD.set(crate::perf_histogram::histogram_method);
        HISTOGRAM_PROPERTY.set(crate::perf_histogram::histogram_property);
        ATTACH_PERF_HOOKS_CONSTRUCTOR.set(crate::perf_hooks::attach_perf_hooks_constructor);
        CONSTRUCT_PERF_HOOKS_CLASS.set(crate::perf_hooks::construct_perf_hooks_class);
        CURRENT_LIST_GET_BY_NAME.set(crate::perf_hooks::current_list_get_by_name);
        CURRENT_LIST_GET_BY_TYPE.set(crate::perf_hooks::current_list_get_by_type);
        CURRENT_LIST_GET_ENTRIES.set(crate::perf_hooks::current_list_get_entries);
        IS_NODE_TIMING_OBJECT.set(crate::perf_hooks::is_node_timing_object);
        IS_PERF_ENTRY_OBJECT.set(crate::perf_hooks::is_perf_entry_object);
        IS_PERF_ENTRY_OBJECT_INSTANCE_OF.set(crate::perf_hooks::is_perf_entry_object_instance_of);
        IS_PERF_HOOKS_SHAPE_INSTANCE_OF.set(crate::perf_hooks::is_perf_hooks_shape_instance_of);
        IS_PERF_OBSERVER_LIST_VALUE.set(crate::perf_hooks::is_perf_observer_list_value);
        IS_PERFORMANCE_NAMESPACE_VALUE.set(crate::perf_hooks::is_performance_namespace_value);
        JS_PERF_CLEAR_MARKS.set(crate::perf_hooks::js_perf_clear_marks);
        JS_PERF_CLEAR_MEASURES.set(crate::perf_hooks::js_perf_clear_measures);
        JS_PERF_CLEAR_RESOURCE_TIMINGS.set(crate::perf_hooks::js_perf_clear_resource_timings);
        JS_PERF_EVENT_LOOP_UTILIZATION.set(crate::perf_hooks::js_perf_event_loop_utilization);
        JS_PERF_GET_ENTRIES.set(crate::perf_hooks::js_perf_get_entries);
        JS_PERF_GET_ENTRIES_BY_NAME.set(crate::perf_hooks::js_perf_get_entries_by_name);
        JS_PERF_GET_ENTRIES_BY_TYPE.set(crate::perf_hooks::js_perf_get_entries_by_type);
        JS_PERF_ILLEGAL_CONSTRUCTOR.set(crate::perf_hooks::js_perf_illegal_constructor);
        JS_PERF_MARK.set(crate::perf_hooks::js_perf_mark);
        JS_PERF_MARK_RESOURCE_TIMING.set(crate::perf_hooks::js_perf_mark_resource_timing);
        JS_PERF_MEASURE.set(crate::perf_hooks::js_perf_measure);
        JS_PERF_NODE_TIMING.set(crate::perf_hooks::js_perf_node_timing);
        JS_PERF_OBSERVER_DISCONNECT.set(crate::perf_hooks::js_perf_observer_disconnect);
        JS_PERF_OBSERVER_OBSERVE.set(crate::perf_hooks::js_perf_observer_observe);
        JS_PERF_OBSERVER_TAKE_RECORDS.set(crate::perf_hooks::js_perf_observer_take_records);
        JS_PERF_SET_RESOURCE_TIMING_BUFFER_SIZE
            .set(crate::perf_hooks::js_perf_set_resource_timing_buffer_size);
        JS_PERF_TIMERIFY.set(crate::perf_hooks::js_perf_timerify);
        JS_PERF_TO_JSON.set(crate::perf_hooks::js_perf_to_json);
        NODE_TIMING_TO_JSON.set(crate::perf_hooks::node_timing_to_json);
        NOTE_EVENT_LOOP_START.set(crate::perf_hooks::note_event_loop_start);
        PERF_ENTRY_TO_JSON.set(crate::perf_hooks::perf_entry_to_json);
        PERF_SUPPORTED_ENTRY_TYPES_VALUE.set(crate::perf_hooks::perf_supported_entry_types_value);
        PERFORMANCE_NAMESPACE.set(crate::perf_hooks::performance_namespace);
        PERFORMANCE_PROTOTYPE_METHOD_VALUE
            .set(crate::perf_hooks::performance_prototype_method_value);
        VALIDATE_PERF_LIST_FILTER_ARG.set(crate::perf_hooks::validate_perf_list_filter_arg);
        REGISTER_ROOTS.set(register_roots);
    });
    register_installed_roots();
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static HISTOGRAM_RESOLVER: Hook<TlsResolver> = Hook::empty();
pub(crate) fn resolve_histograms(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    HISTOGRAM_RESOLVER
        .get()
        .expect("performance installer must precede histogram storage")(idx)
}
pub(crate) fn install_histogram_resolver(resolve: TlsResolver) {
    HISTOGRAM_RESOLVER.set(resolve);
}

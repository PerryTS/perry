//! Constructor-site replay of an ordinary transition chain. ShapeIds own every
//! key and descriptor; the memo holds only input ShapeIds and hook presence.
use super::birth::{walk_shapes, Kind};
use super::*;
use crate::codegen_abi::LayoutWord;
use crate::object::ObjectHeader;
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Input {
    source: LayoutWord,
    options: LayoutWord,
    option_chain: [LayoutWord; 8],
    option_depth: usize,
    prototypes: [LayoutWord; 8],
    prototype_depth: usize,
    presence: u16,
}
impl Input {
    pub(crate) fn own_max(self) -> bool {
        self.presence & (1 << 8) != 0
    }
}
fn matches_input(a: Input, b: Input) -> bool {
    #[cfg(test)]
    let b = {
        let mut b = b;
        if crate::node_stream::native_hooks::stream_sabotage("birth_presence") {
            b.presence = a.presence;
        }
        b
    };
    a == b
}
#[derive(Clone, Copy)]
struct Birth {
    input: Input,
    shape: LayoutWord,
    count: u32,
    #[allow(dead_code)] // part of the memo identity
    inline: u32,
}
thread_local! {
    static R_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
    static W_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
    static D_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
    static T_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
    static P_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
    static E_EXISTING: Cell<Option<Birth>> = const { Cell::new(None) };
}
/// Claim the shape word every birth memo names (full-trace recompute).
pub(crate) fn note_birth_carriers() {
    for site in [
        &R_EXISTING,
        &W_EXISTING,
        &D_EXISTING,
        &T_EXISTING,
        &P_EXISTING,
        &E_EXISTING,
    ] {
        if let Some(b) = site.with(Cell::get) {
            crate::object::shape_carriers::note_shape_id(b.shape);
        }
    }
}
fn existing(kind: Kind) -> &'static std::thread::LocalKey<Cell<Option<Birth>>> {
    match kind {
        Kind::Readable => &R_EXISTING,
        Kind::Writable => &W_EXISTING,
        Kind::Duplex => &D_EXISTING,
        Kind::Transform => &T_EXISTING,
        Kind::PassThrough => &P_EXISTING,
    }
}
const FIELDS: &[&[u8]] = &[
    b"_events",
    b"_readableState",
    b"_writableState",
    b"allowHalfOpen",
    b"_read",
    b"_write",
    b"_writev",
    b"_destroy",
    b"_final",
    b"_construct",
    b"_transform",
    b"_flush",
    b"_maxListeners",
    b"_eventsCount",
];
const OPTIONS: &[&[u8]] = &[
    b"read",
    b"write",
    b"writev",
    b"destroy",
    b"final",
    b"construct",
    b"transform",
    b"flush",
    b"objectMode",
    b"readableObjectMode",
    b"writableObjectMode",
    b"highWaterMark",
    b"readableHighWaterMark",
    b"writableHighWaterMark",
    b"encoding",
    b"defaultEncoding",
    b"emitClose",
    b"autoDestroy",
    b"decodeStrings",
    b"signal",
    b"captureRejections",
    b"allowHalfOpen",
    b"readable",
    b"writable",
];
unsafe fn plain_fields(obj: *mut ObjectHeader, names: &[&[u8]], write: bool) -> bool {
    let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
        return false;
    };
    if !matches!(
        shape.object_kind,
        crate::object::shapes::ShapeObjectKind::Ordinary
            | crate::object::shapes::ShapeObjectKind::OrdinaryUnmarked
            | crate::object::shapes::ShapeObjectKind::Class
    ) {
        return false;
    }
    if !write && shape.object_kind == crate::object::shapes::ShapeObjectKind::Class {
        return false;
    }
    let keys = shape.keys_view();
    names.iter().all(|name| {
        let Some(slot) =
            crate::object::keys_find_property_slot_by_bytes(keys.arr(), keys.count(), name)
        else {
            return true;
        };
        let entry = crate::object::key_attrs::keys_entry(keys.arr(), slot);
        entry & crate::object::key_attrs::ENTRY_ACCESSOR == 0
            && (!write || entry & crate::object::key_attrs::ENTRY_NON_WRITABLE == 0)
    })
}
fn plain_chain(mut bits: u64, names: &[&[u8]], write: bool) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    for _ in 0..8 {
        if bits == TAG_NULL || bits == TAG_UNDEFINED {
            return true;
        }
        let Some(obj) =
            crate::object::field_get_set::runtime_read_site::object_receiver(f64::from_bits(bits))
        else {
            return false;
        };
        if !unsafe { plain_fields(obj, names, write) } {
            return false;
        }
        bits = unsafe { crate::object::shapes::object_prototype_word(obj) };
        if bits == 0 {
            bits = crate::object::js_object_get_prototype_of(box_pointer(obj.cast())).to_bits();
        }
    }
    false
}
#[inline(never)]
fn input(source: Option<f64>, proto: f64, opts: f64, previous: Option<Birth>) -> Option<Input> {
    let _no_move = crate::gc::GcSuppressScope::new();
    let source_shape = source
        .map(|s| unsafe {
            crate::object::shapes::object_shape_stamp(
                crate::object::field_get_set::runtime_read_site::object_receiver(s).unwrap(),
            )
        })
        .unwrap_or(LayoutWord(0));
    let (prototypes, prototype_depth) = walk_shapes(proto.to_bits(), None)?;
    let (options, option_chain, option_depth) =
        if opts.to_bits() == TAG_UNDEFINED || opts.to_bits() == TAG_NULL {
            (LayoutWord(0), [LayoutWord(0); 8], 0)
        } else {
            let obj = crate::object::field_get_set::runtime_read_site::object_receiver(opts)?;
            let options = unsafe { crate::object::shapes::object_shape_stamp(obj) };
            let bits = crate::object::js_object_get_prototype_of(opts).to_bits();
            let (chain, depth) = walk_shapes(bits, None)?;
            (options, chain, depth)
        };
    let proven = previous.is_some_and(|b| {
        b.input.source == source_shape
            && b.input.options == options
            && b.input.option_chain == option_chain
            && b.input.option_depth == option_depth
            && b.input.prototypes == prototypes
            && b.input.prototype_depth == prototype_depth
    });
    if !proven {
        if !plain_chain(proto.to_bits(), FIELDS, true) {
            return None;
        }
        if let Some(s) = source {
            if !unsafe { plain_fields(object_ptr_from_value(s)?, FIELDS, true) } {
                return None;
            }
        }
        if options != LayoutWord(0) {
            if !plain_chain(opts.to_bits(), OPTIONS, false) {
                return None;
            }
        }
    }
    // Abort attachment can call user hooks before the body finishes. It must
    // retain source-ordered publication through the same ordinary Set helper.
    if crate::value::js_is_truthy(crate::runtime_state_key!(b"signal").read_value(opts)) != 0 {
        return None;
    }
    let hooks = [
        crate::runtime_state_key!(b"read"),
        crate::runtime_state_key!(b"write"),
        crate::runtime_state_key!(b"writev"),
        crate::runtime_state_key!(b"destroy"),
        crate::runtime_state_key!(b"final"),
        crate::runtime_state_key!(b"construct"),
        crate::runtime_state_key!(b"transform"),
        crate::runtime_state_key!(b"flush"),
    ];
    let mut presence = 0;
    for (i, hook) in hooks.into_iter().enumerate() {
        if is_callable_value(hook.read_value(opts)) {
            presence |= 1 << i;
        }
    }
    let max = proven_data_value(source, proto, crate::runtime_state_key!(b"_maxListeners"));
    if crate::value::js_is_truthy(max) == 0 {
        presence |= 1 << 8;
    }
    Some(Input {
        source: source_shape,
        options,
        option_chain,
        option_depth,
        prototypes,
        prototype_depth,
        presence,
    })
}
/// Presence validation needs only a proven ordinary data value. Reuse the
/// existing own-slot reader on the source and its already-validated chain;
/// inherited accessors have refused the proof before reaching this helper.
#[inline(never)]
fn proven_data_value(
    source: Option<f64>,
    proto: f64,
    key: crate::runtime_state_key::NamedStateKey,
) -> f64 {
    let _no_move = crate::gc::GcSuppressScope::new();
    if let Some(source) = source {
        if let Some(obj) = crate::object::field_get_set::runtime_read_site::object_receiver(source)
        {
            if let Some(value) = unsafe { key.read_own(obj) } {
                return value;
            }
        }
    }
    let mut bits = proto.to_bits();
    for _ in 0..8 {
        let Some(obj) =
            crate::object::field_get_set::runtime_read_site::object_receiver(f64::from_bits(bits))
        else {
            break;
        };
        if let Some(value) = unsafe { key.read_own(obj) } {
            return value;
        }
        bits = unsafe { crate::object::shapes::object_prototype_word(obj) };
        if bits == 0 {
            bits = crate::object::js_object_get_prototype_of(box_pointer(obj.cast())).to_bits();
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}
/// Read the same proven ordinary data chain after a constructor has
/// normalized options. Input rejects every accessor/write interception on
/// these fields, so publication cannot run user code on a replayed birth.
#[inline(never)]
pub(crate) fn read_field(
    stream: f64,
    key: crate::runtime_state_key::NamedStateKey,
    proof: Option<Input>,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    if let Some(proof) = proof {
        let object = object_ptr_from_value(stream.get_nanbox_f64()).expect("stream receiver");
        let proto = unsafe { crate::object::shapes::object_prototype_word(object) };
        let proto = if proto == 0 {
            crate::object::js_object_get_prototype_of(stream.get_nanbox_f64())
        } else {
            f64::from_bits(proto)
        };
        if walk_shapes(proto.to_bits(), None) == Some((proof.prototypes, proof.prototype_depth)) {
            return proven_data_value(Some(stream.get_nanbox_f64()), proto, key);
        }
        key.read_value(stream.get_nanbox_f64())
    } else {
        key.read_value(stream.get_nanbox_f64())
    }
}
fn remember(site: &'static std::thread::LocalKey<Cell<Option<Birth>>>, input: Input, stream: f64) {
    let obj = object_ptr_from_value(stream).unwrap();
    let shape = unsafe { crate::object::shapes::object_shape_stamp(obj) };
    let Some(record) = crate::object::shapes::shape_descriptor_for_word(shape) else {
        return;
    };
    if record.hole_count != 0 || record.integrity_flags != 0 {
        return;
    }
    crate::object::shape_carriers::note_shape_id(shape);
    site.with(|m| {
        m.set(Some(Birth {
            input,
            shape,
            count: record.logical_key_count,
            inline: record.live_inline_slot_count,
        }))
    });
}
/// Direct allocation replays the exact final ordinary transition chain. The
/// body still normalizes options and fills its slots in the original order.
#[inline(never)]
fn seed_new_count(stream: f64, proto: f64, source_shape: Option<LayoutWord>) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let proto = scope.root_nanbox_f64(proto);
    let count = crate::runtime_state_key!(b"_eventsCount");
    let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
    if let Some(from) = source_shape {
        let Some(shape) = crate::object::shapes::shape_descriptor_for_word(from) else {
            return;
        };
        if unsafe {
            crate::object::keys_find_property_slot_by_bytes(
                shape.keys_view().arr(),
                shape.logical_key_count,
                count.bytes,
            )
        }
        .is_some()
        {
            return;
        }
    }
    if unsafe { count.read_own(obj) }.is_some() {
        let initial = scope.root_nanbox_f64(count.read_value(proto.get_nanbox_f64()));
        count.write_value(stream.get_nanbox_f64(), initial.get_nanbox_f64());
    }
}
#[inline(never)]
fn begin(
    site: &'static std::thread::LocalKey<Cell<Option<Birth>>>,
    stream: f64,
    opts: f64,
) -> Option<Input> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let opts = scope.root_nanbox_f64(opts);
    let proto = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
        stream.get_nanbox_f64(),
    ));
    let previous = site.with(Cell::get);
    let proof = input(
        Some(stream.get_nanbox_f64()),
        proto.get_nanbox_f64(),
        opts.get_nanbox_f64(),
        previous,
    )?;
    if let Some(b) = previous.filter(|b| matches_input(b.input, proof)) {
        if let Some(shape) = crate::object::shapes::shape_descriptor_for_word(b.shape) {
            let obj = object_ptr_from_value(stream.get_nanbox_f64()).unwrap();
            if unsafe {
                crate::object::shapes::install_cached_object_shape_transition(
                    obj,
                    proof.source,
                    b.shape,
                    shape.keys_view(),
                )
            } {
                crate::object::reserve_object_spill(obj as usize, b.count);
                seed_new_count(
                    stream.get_nanbox_f64(),
                    proto.get_nanbox_f64(),
                    Some(proof.source),
                );
            }
        }
    }
    Some(proof)
}
pub(super) fn begin_existing(stream: f64, opts: f64, kind: Kind) -> Option<Input> {
    begin(existing(kind), stream, opts)
}
pub(super) fn finish_existing(stream: f64, proof: Option<Input>, kind: Kind) {
    if let Some(proof) = proof {
        remember(existing(kind), proof, stream);
    }
}
pub(super) fn begin_emitter(stream: f64, opts: f64) -> Option<Input> {
    begin(&E_EXISTING, stream, opts)
}
pub(super) fn finish_emitter(stream: f64, proof: Option<Input>) {
    if let Some(proof) = proof {
        remember(&E_EXISTING, proof, stream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F2 #3: a birth memo's shape word survives full traces with no stream
    /// on it; fails if the recompute does not re-note it.
    #[test]
    fn birth_memo_shape_is_carried_across_full_traces() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let input = Input {
            source: LayoutWord(0),
            options: LayoutWord(0),
            option_chain: [LayoutWord(0); 8],
            option_depth: 0,
            prototypes: [LayoutWord(0); 8],
            prototype_depth: 0,
            presence: 0,
        };
        let shape = {
            let obj = crate::object::js_object_alloc(0, 0);
            unsafe { crate::object::shapes::transition_object_shape_semantics(obj) };
            box_pointer(obj.cast())
        };
        remember(&P_EXISTING, input, shape);
        let word = P_EXISTING.with(Cell::get).expect("memo recorded").shape;
        let control = {
            let obj = crate::object::js_object_alloc(0, 0);
            unsafe { crate::object::shapes::transition_object_shape_semantics(obj) }
        };
        for round in 0..3 {
            crate::gc::test_collect_synchronous_full_trace();
            assert!(
                crate::object::shapes::shape_is_retired(control),
                "control must retire"
            );
            assert!(
                !crate::object::shapes::shape_is_retired(word),
                "round {round}: birth memo shape retired"
            );
        }
    }
    extern "C" fn read_hook(_: *const ClosureHeader, _: crate::closure::JsThis, _: f64) -> f64 {
        f64::from_bits(TAG_UNDEFINED)
    }
    // A direct birth on a layout replays nothing: an option dropped between
    // births leaves no own hook behind.
    #[test]
    fn warm_presence_change_does_not_keep_an_own_hook() {
        let scope = crate::gc::RuntimeHandleScope::new();
        let opts =
            scope.root_nanbox_f64(box_pointer(crate::object::js_object_alloc_plain(2).cast()));
        let hook = scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(read_hook, 1), 0));
        super::super::birth::ordinary_birth_set(
            opts.get_nanbox_f64(),
            crate::runtime_state_key!(b"read"),
            box_pointer(hook.get_raw_mut_ptr::<ClosureHeader>().cast()),
        );
        let first = scope.root_nanbox_f64(crate::node_stream::js_node_stream_passthrough_new(
            opts.get_nanbox_f64(),
        ));
        let _second = scope.root_nanbox_f64(crate::node_stream::js_node_stream_passthrough_new(
            opts.get_nanbox_f64(),
        ));
        let old_shape = unsafe {
            crate::object::shapes::object_shape_stamp(
                object_ptr_from_value(opts.get_nanbox_f64()).unwrap(),
            )
        };
        crate::runtime_state_key!(b"read")
            .write_value(opts.get_nanbox_f64(), f64::from_bits(TAG_UNDEFINED));
        assert_eq!(old_shape, unsafe {
            crate::object::shapes::object_shape_stamp(
                object_ptr_from_value(opts.get_nanbox_f64()).unwrap(),
            )
        });
        let changed = scope.root_nanbox_f64(crate::node_stream::js_node_stream_passthrough_new(
            opts.get_nanbox_f64(),
        ));
        let key = crate::runtime_state_key!(b"_read");
        assert!(
            unsafe { key.read_own(object_ptr_from_value(first.get_nanbox_f64()).unwrap()) }
                .is_some()
        );
        assert!(
            unsafe { key.read_own(object_ptr_from_value(changed.get_nanbox_f64()).unwrap()) }
                .is_none(),
            "same option shape with absent hook must select the base shape"
        );
    }
}

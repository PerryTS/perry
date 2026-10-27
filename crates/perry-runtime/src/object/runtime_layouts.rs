//! Runtime declarations using the step-0 ABI, shared with emitted code.
use super::layout_desc::{PerryLayoutDesc, PerryLayoutKey, StaticAtom};
use crate::gc::GcHeader;
use crate::string::StringHeader;

// The symbol suffix is explicit because macro_rules cannot hex-encode a string
// into an export name. The bytes themselves occur once, with the image's NUL
// and the FNV hash both derived here. Existing runtime atoms are reused.
macro_rules! runtime_atom {
    (reuse $name:ident, $bytes:literal, $symbol:literal) => {
        #[allow(non_snake_case)]
        pub(crate) mod $name {
            pub(crate) const HASH: u64 = super::atom_hash($bytes.as_bytes());
        }
        unsafe extern "C" {
            #[link_name = concat!("perry_atom_", $symbol)]
            pub(crate) safe static $name: StaticAtom;
        }
    };
    ($name:ident, $bytes:literal, $symbol:literal) => {
        #[allow(non_snake_case)]
        pub(crate) mod $name {
            use super::*;
            pub(crate) const HASH: u64 = atom_hash($bytes.as_bytes());
            #[used(linker)]
            #[linkage = "weak_odr"]
            #[export_name = concat!("perry_image_literal_0_", $symbol)]
            #[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
            #[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
            #[cfg_attr(
                not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
                link_section = "perry_strings"
            )]
            pub(super) static IMAGE: ImageString<{ $bytes.len() + 1 }> =
                image_bytes(concat!($bytes, "\0").as_bytes());
            wasm_string_image!(IMAGE);
        }
        #[used(linker)]
        #[linkage = "weak_odr"]
        #[export_name = concat!("perry_atom_", $symbol)]
        #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
        #[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = "perry_atoms"
        )]
        pub(crate) static $name: StaticAtom = StaticAtom {
            hash: $name::HASH,
            string: std::ptr::addr_of!($name::IMAGE.string),
            kind: crate::codegen_abi::ATOM_KIND_STRING,
            _pad: [0; 3],
            index: 0,
        };
        wasm_atom!($name);
    };
}

const fn image_bytes<const N: usize>(bytes: &[u8]) -> ImageString<N> {
    assert!(bytes.len() == N);
    let mut result = [0; N];
    let mut i = 0;
    while i < N {
        result[i] = bytes[i];
        i += 1;
    }
    image(result)
}

// Atom, hash, attribute byte, storage slot (`None`: the key's position).
type LayoutAtom = (*const StaticAtom, u64, u8, Option<u32>);

const fn layout_keys<const N: usize>(atoms: [LayoutAtom; N]) -> [PerryLayoutKey; N] {
    let mut keys = [const {
        PerryLayoutKey {
            atom: std::ptr::null(),
            slot: 0,
            attrs: 0,
            _pad: [0; 3],
        }
    }; N];
    let mut taken = [false; N];
    let mut i = 0;
    while i < N {
        let slot = match atoms[i].3 {
            Some(slot) => slot,
            None => i as u32,
        };
        // Storage slots are a permutation of the key positions.
        assert!((slot as usize) < N && !taken[slot as usize]);
        taken[slot as usize] = true;
        keys[i] = PerryLayoutKey {
            atom: atoms[i].0,
            slot,
            attrs: atoms[i].2,
            _pad: [0; 3],
        };
        i += 1;
    }
    keys
}

const fn layout_index_size(keys: usize) -> usize {
    (keys * 2).next_power_of_two()
}

const fn layout_index<const N: usize>(atoms: &[LayoutAtom]) -> [u32; N] {
    assert!(N == layout_index_size(atoms.len()));
    let mut index = [u32::MAX; N];
    let mut i = 0;
    while i < atoms.len() {
        let mut bucket = atoms[i].1 as usize & (N - 1);
        while index[bucket] != u32::MAX {
            bucket = (bucket + 1) & (N - 1);
        }
        index[bucket] = i as u32;
        i += 1;
    }
    index
}

// Optional attribute bytes support the non-default checked contracts; the
// family/variant are carried contract facts. The key order is the
// enumeration order; `ATOM @ slot` stores a key in another slot (default:
// its position). Padding and probing data are computed, including the
// one-bucket empty layout; the index holds key positions.
macro_rules! runtime_layout {
    (@attrs) => { 0 };
    (@attrs $attrs:expr) => { $attrs };
    (@slot) => { None };
    (@slot $slot:expr) => { Some($slot) };
    ($name:ident, $family:expr, $variant:expr, $proto:expr, [$($atom:ident $(@ $slot:expr)? $(=> $attrs:expr)?),* $(,)?]) => {
        runtime_layout!($name, kind = 0, $family, $variant, $proto, [$($atom $(@ $slot)? $(=> $attrs)?),*]);
    };
    // `kind`: the layout's `ShapeObjectKind` code (an ordinary-family kind).
    ($name:ident, kind = $kind:expr, $family:expr, $variant:expr, $proto:expr, [$($atom:ident $(@ $slot:expr)? $(=> $attrs:expr)?),* $(,)?]) => {
        #[allow(non_snake_case)]
        pub mod $name {
            use super::*;
            const ATOMS: &[LayoutAtom] = &[$((std::ptr::addr_of!($atom), $atom::HASH, runtime_layout!(@attrs $($attrs)?), runtime_layout!(@slot $($slot)?))),*];
            pub const SLOT_COUNT: usize = ATOMS.len();
            pub(super) static KEYS: [PerryLayoutKey; SLOT_COUNT] =
                layout_keys([$((std::ptr::addr_of!($atom), $atom::HASH, runtime_layout!(@attrs $($attrs)?), runtime_layout!(@slot $($slot)?))),*]);
            pub(super) static INDEX: [u32; layout_index_size(SLOT_COUNT)] = layout_index(ATOMS);
        }
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
        #[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
        #[cfg_attr(not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")), link_section = "perry_layouts")]
        #[export_name = concat!("perry_runtime_layout_", stringify!($name))]
        /// Declared layout of a checked runtime record.
        pub static mut $name: PerryLayoutDesc = PerryLayoutDesc {
            shape_id: 0, _layout_pad: 0,
            family: $family, variant: $variant, kind: $kind, integrity: 0,
            slot_count: $name::SLOT_COUNT as u32,
            layout_id: crate::object::runtime_layouts::runtime_layout_word(stringify!($name)),
            rep: 0, proto: $proto, keys: $name::KEYS.as_ptr(),
            key_count: $name::SLOT_COUNT as u32,
            index_mask: (layout_index_size($name::SLOT_COUNT) - 1) as u32,
            index: $name::INDEX.as_ptr(),
            class_id: 0, parent_class_id: 0, methods: std::ptr::null(),
        };
wasm_layout!($name);
    };
}

/// The runtime prototypes runtime layouts name (object model §3.6), each a
/// family whose serial is its reserved intrinsic one. The runtime gives the
/// prototype that serial when it builds it.
macro_rules! runtime_prototype_family {
    ($($name:ident = $serial:ident),* $(,)?) => {$(
        pub(crate) static mut $name: crate::object::layout_desc::PerryProtoFamily =
            crate::object::layout_desc::PerryProtoFamily { serial: crate::closure::shape::$serial };
    )*};
}
runtime_prototype_family!(
    URL_PROTO_FAMILY = INTRINSIC_SERIAL_URL,
    READABLE_STATE_PROTO_FAMILY = INTRINSIC_SERIAL_READABLE_STATE,
    WRITABLE_STATE_PROTO_FAMILY = INTRINSIC_SERIAL_WRITABLE_STATE,
    TIMEOUT_PROTO_FAMILY = INTRINSIC_SERIAL_TIMEOUT,
    IMMEDIATE_PROTO_FAMILY = INTRINSIC_SERIAL_IMMEDIATE,
    URL_SEARCH_PARAMS_PROTO_FAMILY = INTRINSIC_SERIAL_URL_SEARCH_PARAMS,
);

include!("runtime_layouts/records.rs");
include!("runtime_layouts/ascii_chars.rs");

/// The qualified private keys of the runtime's intrinsic slots
/// (`private_storage_spelling(0, 0, name)`), as static atoms. A builtin's
/// brand read (`IntrinsicPrivateReadSite`) compares this one key word against
/// the receiver's layout: no spelling is built, hashed or looked up per read.
pub(crate) mod intrinsic_private_atoms {
    use super::*;
    runtime_atom!(
        REGEXP_MATCHER,
        "#<perry:private-value:0:@0:[[RegExpMatcher]]>",
        "233c70657272793a707269766174652d76616c75653a303a40303a5b5b5265674578704d6174636865725d5d3e"
    );
    runtime_atom!(
        REGEXP_PROTOTYPE,
        "#<perry:private-value:0:@0:[[RegExpPrototype]]>",
        "233c70657272793a707269766174652d76616c75653a303a40303a5b5b52656745787050726f746f747970655d5d3e"
    );
    // A sync generator instance's resume record (`generator_state`).
    runtime_atom!(
        GENERATOR_STATE,
        "#<perry:private-value:0:@0:[[GeneratorState]]>",
        "233c70657272793a707269766174652d76616c75653a303a40303a5b5b47656e657261746f7253746174655d5d3e"
    );
    // A generator function's original prototype, in its bag
    // (`closure::props`).
    runtime_atom!(
        GENERATOR_PROTOTYPE,
        "#<perry:private-value:0:@0:[[GeneratorPrototype]]>",
        "233c70657272793a707269766174652d76616c75653a303a40303a5b5b47656e657261746f7250726f746f747970655d5d3e"
    );
    #[cfg(test)]
    runtime_atom!(
        COLD_STORE,
        "#<perry:private-value:0:@0:coldStore>",
        "233c70657272793a707269766174652d76616c75653a303a40303a636f6c6453746f72653e"
    );
}
include!("runtime_layouts/builtin_prototypes.rs");
include!("runtime_layouts/node_prototypes.rs");
include!("runtime_layouts/node_prototype_birth.rs");

/// `child_process.spawnSync`'s result. Node gives it `error` first, and only
/// when the spawn failed: the key is an optional member (object model §3.2),
/// so a successful run leaves its slot holding the hole and the key is absent
/// (`key_view`). Every other member is always present.
mod spawn_sync_result {
    use super::record_atoms::*;
    use super::*;
    runtime_atom!(SPAWN_SYNC_ATOM_ERROR, "error", "6572726f72");
    runtime_atom!(SPAWN_SYNC_ATOM_OUTPUT, "output", "6f7574707574");
    runtime_atom!(SPAWN_SYNC_ATOM_PID, "pid", "706964");
    runtime_atom!(SPAWN_SYNC_ATOM_STDOUT, "stdout", "7374646f7574");
    runtime_atom!(SPAWN_SYNC_ATOM_STDERR, "stderr", "737464657272");
    runtime_layout!(SPAWN_SYNC_RESULT, 0x5350_5359, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
        SPAWN_SYNC_ATOM_ERROR => crate::object::key_attrs::ENTRY_OPTIONAL,
        RECORD_ATOM_88,
        RECORD_ATOM_189,
        SPAWN_SYNC_ATOM_OUTPUT,
        SPAWN_SYNC_ATOM_PID,
        SPAWN_SYNC_ATOM_STDOUT,
        SPAWN_SYNC_ATOM_STDERR,
    ]);
    /// The slots of [`SPAWN_SYNC_RESULT`].
    pub(crate) mod spawn_sync_slot {
        pub(crate) const ERROR: u32 = 0;
        pub(crate) const STATUS: u32 = 1;
        pub(crate) const SIGNAL: u32 = 2;
        pub(crate) const OUTPUT: u32 = 3;
        pub(crate) const PID: u32 = 4;
        pub(crate) const STDOUT: u32 = 5;
        pub(crate) const STDERR: u32 = 6;
    }
}
pub(crate) use spawn_sync_result::spawn_sync_slot;
pub use spawn_sync_result::SPAWN_SYNC_RESULT;

/// The former runtime CommonJS module record layout. Records are compiler
/// layouts now (the wrapper's record literal) and nothing allocates this one.
/// Removing it makes `tombstone_tests::an_old_receiver_keeps_its_overflow_across_a_minor`
/// lose an old receiver's young overflow in a minor collection: a latent
/// collector dependency on the static layout set, open and separate.
mod cjs_module_record {
    use super::record_atoms::*;
    use super::*;
    runtime_atom!(CJS_ATOM_EXPORTS, "exports", "6578706f727473");
    runtime_atom!(
        CJS_ATOM_RECORD,
        "__perry_cjs_record",
        "5f5f70657272795f636a735f7265636f7264"
    );
    runtime_atom!(CJS_ATOM_ID, "id", "6964");
    runtime_atom!(CJS_ATOM_LOADED, "loaded", "6c6f61646564");
    runtime_atom!(CJS_ATOM_CHILDREN, "children", "6368696c6472656e");
    runtime_atom!(CJS_ATOM_PARENT, "parent", "706172656e74");
    runtime_atom!(CJS_ATOM_PATHS, "paths", "7061746873");
    runtime_atom!(CJS_ATOM_REQUIRE, "require", "72657175697265");
    runtime_atom!(
        CJS_ATOM_FACTORY,
        "#<perry:cjs-module-factory>",
        "233c70657272793a636a732d6d6f64756c652d666163746f72793e"
    );
    runtime_layout!(CJS_MODULE_RECORD, 0x434A_534D, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
        CJS_ATOM_EXPORTS,
        CJS_ATOM_RECORD,
        CJS_ATOM_ID,
        RECORD_ATOM_9,
        RECORD_ATOM_184,
        CJS_ATOM_LOADED,
        CJS_ATOM_CHILDREN,
        CJS_ATOM_PARENT,
        CJS_ATOM_PATHS,
        CJS_ATOM_REQUIRE,
        CJS_ATOM_FACTORY => crate::object::key_attrs::PRIVATE_FIELD_ENTRY,
    ]);
}
pub use cjs_module_record::CJS_MODULE_RECORD;

/// Allocate a fixed record with every declared property present, including undefined values.
///
/// # Safety
/// `desc` is a registered descriptor of this binary's fixed runtime records.
#[no_mangle]
pub unsafe extern "C" fn perry_alloc_runtime_record(
    desc: *const PerryLayoutDesc,
) -> *mut super::ObjectHeader {
    let object = super::layout_desc::perry_alloc_layout(desc);
    let fields = object
        .cast::<u8>()
        .add(std::mem::size_of::<super::ObjectHeader>())
        .cast::<u64>();
    // GC_STORE_AUDIT(INIT): unpublished record; undefined words have no managed edges.
    for slot in 0..(*desc).slot_count as usize {
        *fields.add(slot) = crate::value::TAG_UNDEFINED;
    }
    object
}

// A hygienic stack binding for each builder, sized by its declaration.
macro_rules! runtime_record {
    ($object:ident, $desc:ident) => {
        let fields = [const { std::cell::UnsafeCell::new(f64::from_bits(crate::value::TAG_UNDEFINED)) };
            crate::object::runtime_layouts::$desc::SLOT_COUNT];
        let $object = unsafe { crate::object::runtime_layouts::RuntimeRecordBuilder::new(
            std::ptr::addr_of!(crate::object::runtime_layouts::$desc), &fields) };
    };
}
pub(crate) use runtime_record;

/// One shared record birth: root the value buffer before allocation, then fill
/// declared slots through the ordinary barrier funnel. Moving GC updates the
/// cells themselves; no copied managed pointer survives allocation.
///
/// # Safety
/// `desc` is registered, and `values` points to `slot_count` writable NaN-boxed
/// words which remain at this address until the call returns.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn perry_build_runtime_record(
    desc: *const PerryLayoutDesc,
    values: *mut f64,
) -> *mut super::ObjectHeader {
    let roots = crate::gc::RuntimeHandleScope::new();
    let count = (*desc).slot_count as usize;
    let cells = std::slice::from_raw_parts(values.cast::<std::cell::UnsafeCell<f64>>(), count);
    for cell in cells {
        roots.root_nanbox_cell(cell);
    }
    let object = roots.root_raw_mut_ptr(super::layout_desc::perry_alloc_layout(desc));
    for (slot, cell) in cells.iter().enumerate() {
        // The field-store funnel does not allocate or poll the collector.
        super::js_object_set_field(
            object.get_raw_mut_ptr(),
            slot as u32,
            crate::JSValue::from_bits((*cell.get()).to_bits()),
        );
    }
    object.get_raw_mut_ptr()
}

#[used(linker)]
static KEEP_RUNTIME_RECORD_BUILD: unsafe extern "C" fn(
    *const PerryLayoutDesc,
    *mut f64,
) -> *mut super::ObjectHeader = perry_build_runtime_record;

/// Values are evaluated in caller order into a borrowed, fixed stack buffer.
/// Registering the cells once lets every later allocation rewrite that buffer.
/// The borrow prevents it from moving or being destroyed while rooted.
pub struct RuntimeRecordBuilder<'a> {
    desc: *const PerryLayoutDesc,
    fields: &'a [std::cell::UnsafeCell<f64>],
    _roots: std::mem::ManuallyDrop<crate::gc::RuntimeHandleScope>,
}

impl<'a> RuntimeRecordBuilder<'a> {
    /// # Safety
    /// `desc` is registered and its slot count equals the buffer length.
    #[inline(never)]
    pub unsafe fn new(
        desc: *const PerryLayoutDesc,
        fields: &'a [std::cell::UnsafeCell<f64>],
    ) -> Self {
        assert_eq!((*desc).slot_count as usize, fields.len());
        let roots = crate::gc::RuntimeHandleScope::new();
        for cell in fields {
            roots.root_nanbox_cell(cell);
        }
        Self {
            desc,
            fields,
            _roots: std::mem::ManuallyDrop::new(roots),
        }
    }

    #[inline(never)]
    pub fn set(&self, slot: u32, value: crate::JSValue) {
        unsafe {
            *self.fields[slot as usize].get() = f64::from_bits(value.bits());
        }
        crate::gc::runtime_write_barrier_root_nanbox(value.bits());
    }

    #[inline(never)]
    pub fn get(&self, slot: u32) -> crate::JSValue {
        unsafe { crate::JSValue::from_bits((*self.fields[slot as usize].get()).to_bits()) }
    }

    #[inline(never)]
    pub fn finish(&self) -> *mut super::ObjectHeader {
        unsafe { perry_build_runtime_record(self.desc, self.fields.as_ptr().cast_mut().cast()) }
    }
}

impl Drop for RuntimeRecordBuilder<'_> {
    #[inline(never)]
    fn drop(&mut self) {
        unsafe {
            std::mem::ManuallyDrop::drop(&mut self._roots);
        }
    }
}

/// Initialize a declared slot after the value expression has finished allocating.
#[inline(never)]
pub fn init_runtime_field(object: &crate::gc::RuntimeHandle<'_>, slot: u32, value: crate::JSValue) {
    super::js_object_set_field(object.get_raw_mut_ptr::<super::ObjectHeader>(), slot, value);
}

#[used(linker)]
static KEEP_RUNTIME_RECORD_ALLOC: unsafe extern "C" fn(
    *const PerryLayoutDesc,
) -> *mut super::ObjectHeader = perry_alloc_runtime_record;

/// Build the shared fixed image record and its resolved promise for native UI backends.
#[no_mangle]
pub extern "C" fn perry_runtime_image_result(handle: i64, width: f64, height: f64) -> i64 {
    runtime_record!(object, RECORD_U610);
    object.set(0, crate::JSValue::number(handle as f64));
    object.set(1, crate::JSValue::number(width));
    object.set(2, crate::JSValue::number(height));
    object.set(3, crate::JSValue::bool(true));
    crate::promise::js_promise_resolved(crate::value::js_nanbox_pointer(object.finish() as i64))
        as i64
}

#[used(linker)]
static KEEP_IMAGE_RESULT: extern "C" fn(i64, f64, f64) -> i64 = perry_runtime_image_result;

/// The same object codegen emits for a program literal of this spelling
/// (`string_pool/image.rs`), and under the same symbol, so the program's
/// weak image of a spelling the runtime also names resolves to this one: a
/// property atom (its hash after the NUL-terminated bytes, 8-aligned) that
/// is also program-named, as the program's own image would have been.
#[repr(C, align(8))]
struct ImageString<const N: usize> {
    gc: GcHeader,
    string: StringHeader,
    bytes: [u8; N],
    hash: u64,
}

const fn image<const N: usize>(bytes: [u8; N]) -> ImageString<N> {
    let len = (N - 1) as u32;
    ImageString {
        gc: GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: crate::gc::GC_FLAG_MARKED
                | crate::gc::GC_FLAG_PINNED
                | crate::gc::GC_FLAG_INTERNED,
            _reserved: crate::string::STRING_IMAGE_MARKER,
            size: crate::string::image_string_gc_size(len) as u32,
        },
        string: StringHeader {
            utf16_len: len,
            byte_len: len,
            capacity: len,
            refcount: 0,
            flags: crate::string::STRING_FLAG_WTF8_VALIDATED | crate::string::IMAGE_ATOM_FLAGS,
        },
        bytes,
        hash: atom_hash_prefix(&bytes, N - 1),
    }
}

pub(crate) const fn atom_hash(bytes: &[u8]) -> u64 {
    atom_hash_prefix(bytes, bytes.len())
}

const fn atom_hash_prefix(bytes: &[u8], len: usize) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < len {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    hash
}

#[inline]
pub(crate) unsafe fn layout_id(desc: *const PerryLayoutDesc) -> u32 {
    (*desc).layout_id
}

#[path = "runtime_layouts/literal_atoms.rs"]
mod literal_atoms;
use literal_atoms::ATOM_0;
use literal_atoms::ATOM_1;
use literal_atoms::ATOM_10;
use literal_atoms::ATOM_11;
use literal_atoms::ATOM_12;
use literal_atoms::ATOM_13;
use literal_atoms::ATOM_14;
use literal_atoms::ATOM_15;
use literal_atoms::ATOM_16;
use literal_atoms::ATOM_17;
use literal_atoms::ATOM_18;
use literal_atoms::ATOM_19;
use literal_atoms::ATOM_2;
use literal_atoms::ATOM_20;
use literal_atoms::ATOM_21;
use literal_atoms::ATOM_22;
use literal_atoms::ATOM_23;
use literal_atoms::ATOM_24;
use literal_atoms::ATOM_25;
use literal_atoms::ATOM_26;
use literal_atoms::ATOM_27;
use literal_atoms::ATOM_28;
use literal_atoms::ATOM_29;
use literal_atoms::ATOM_3;
use literal_atoms::ATOM_30;
use literal_atoms::ATOM_4;
use literal_atoms::ATOM_5;
use literal_atoms::ATOM_6;
use literal_atoms::ATOM_7;
use literal_atoms::ATOM_8;
use literal_atoms::ATOM_9;
#[cfg(test)]
use literal_atoms::IMAGE_0;

#[cfg(all(test, target_os = "windows"))]
pub(crate) fn test_image_atom() -> *const StaticAtom {
    std::ptr::addr_of!(ATOM_0)
}

#[cfg(test)]
pub(crate) fn test_image_string() -> *const StringHeader {
    &IMAGE_0.string
}

static READABLE_STATE_KEYS: [PerryLayoutKey; 6] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_1),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_2),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_3),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_4),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_5),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
];
static READABLE_STATE_INDEX: [u32; 16] = [
    u32::MAX,
    2,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    3,
    u32::MAX,
    5,
    u32::MAX,
    0,
    u32::MAX,
    1,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut READABLE_STATE: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540001,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 6,
    layout_id: crate::codegen_abi::native_layout_word("READABLE_STATE", 1),
    rep: 0,
    proto: &raw const READABLE_STATE_PROTO_FAMILY,
    keys: READABLE_STATE_KEYS.as_ptr(),
    key_count: 6,
    index_mask: 15,
    index: READABLE_STATE_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(READABLE_STATE);

static WRITABLE_STATE_KEYS: [PerryLayoutKey; 7] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_3),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_6),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_7),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_8),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_9),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_10),
        slot: 6,
        attrs: 0,
        _pad: [0; 3],
    },
];
static WRITABLE_STATE_INDEX: [u32; 16] = [
    6,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    1,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    5,
    3,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut WRITABLE_STATE: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540002,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 7,
    layout_id: crate::codegen_abi::native_layout_word("WRITABLE_STATE", 1),
    rep: 0,
    proto: &raw const WRITABLE_STATE_PROTO_FAMILY,
    keys: WRITABLE_STATE_KEYS.as_ptr(),
    key_count: 7,
    index_mask: 15,
    index: WRITABLE_STATE_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(WRITABLE_STATE);

static URL_SEARCH_PARAMS_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_11),
        slot: 0,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_12),
        slot: 1,
        attrs: 66,
        _pad: [0; 3],
    },
];
static URL_SEARCH_PARAMS_INDEX: [u32; 4] = [0, 1, u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut URL_SEARCH_PARAMS: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540003,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: crate::codegen_abi::native_layout_word("URL_SEARCH_PARAMS", 1),
    rep: 0,
    proto: &raw const URL_SEARCH_PARAMS_PROTO_FAMILY,
    keys: URL_SEARCH_PARAMS_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: URL_SEARCH_PARAMS_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(URL_SEARCH_PARAMS);

static URL_COMPONENTS_KEYS: [PerryLayoutKey; 12] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_13),
        slot: 0,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_14),
        slot: 1,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_15),
        slot: 2,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_16),
        slot: 3,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_17),
        slot: 4,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_18),
        slot: 5,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_19),
        slot: 6,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_20),
        slot: 7,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_21),
        slot: 8,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_22),
        slot: 9,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_23),
        slot: 10,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_24),
        slot: 11,
        attrs: 66,
        _pad: [0; 3],
    },
];
static URL_COMPONENTS_INDEX: [u32; 32] = [
    3,
    6,
    u32::MAX,
    9,
    7,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    5,
    u32::MAX,
    u32::MAX,
    10,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    8,
    u32::MAX,
    4,
    u32::MAX,
    11,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    0,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut URL_COMPONENTS: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540004,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 12,
    layout_id: crate::codegen_abi::native_layout_word("URL_COMPONENTS", 1),
    rep: 0,
    proto: &raw const URL_PROTO_FAMILY,
    keys: URL_COMPONENTS_KEYS.as_ptr(),
    key_count: 12,
    index_mask: 31,
    index: URL_COMPONENTS_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(URL_COMPONENTS);

static ITERATOR_RESULT_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_25),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_26),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static ITERATOR_RESULT_INDEX: [u32; 4] = [u32::MAX, 1, 0, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ITERATOR_RESULT: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540005,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: crate::codegen_abi::native_layout_word("ITERATOR_RESULT", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: ITERATOR_RESULT_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: ITERATOR_RESULT_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(ITERATOR_RESULT);

static ITERATOR_RESULT_DONE_FIRST_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_26),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_25),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static ITERATOR_RESULT_DONE_FIRST_INDEX: [u32; 4] = [u32::MAX, 0, 1, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ITERATOR_RESULT_DONE_FIRST: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540006,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: crate::codegen_abi::native_layout_word("ITERATOR_RESULT_DONE_FIRST", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: ITERATOR_RESULT_DONE_FIRST_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: ITERATOR_RESULT_DONE_FIRST_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(ITERATOR_RESULT_DONE_FIRST);

static TIMEOUT_KEYS: [PerryLayoutKey; 0] = [];
static TIMEOUT_INDEX: [u32; 2] = [u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut TIMEOUT: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540007,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: crate::codegen_abi::native_layout_word("TIMEOUT", 1),
    rep: 0,
    proto: &raw const TIMEOUT_PROTO_FAMILY,
    keys: TIMEOUT_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 1,
    index: TIMEOUT_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(TIMEOUT);

static IMMEDIATE_KEYS: [PerryLayoutKey; 0] = [];
static IMMEDIATE_INDEX: [u32; 2] = [u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut IMMEDIATE: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540008,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: crate::codegen_abi::native_layout_word("IMMEDIATE", 1),
    rep: 0,
    proto: &raw const IMMEDIATE_PROTO_FAMILY,
    keys: IMMEDIATE_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 1,
    index: IMMEDIATE_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(IMMEDIATE);

pub(crate) const URL_CLASS_ID: u32 = 0xFFFF0063;

mod stream_prototypes;
pub(crate) use stream_prototypes::*;
mod stream_instances;
pub(crate) use stream_instances::*;
mod http_instances;
pub(crate) use http_instances::*;

/// `Symbol.iterator` as a layout key (§6a, a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_well_known_iterator"]
static ATOM_SYMBOL_ITERATOR: StaticAtom = StaticAtom {
    hash: crate::codegen_abi::well_known_symbol_atom_hash(
        crate::codegen_abi::WELL_KNOWN_SYMBOL_ITERATOR,
    ),
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
    _pad: [0; 3],
    index: crate::codegen_abi::WELL_KNOWN_SYMBOL_ITERATOR,
};
wasm_atom!(ATOM_SYMBOL_ITERATOR);
static REGEX_EXEC_NAMED_KEYS: [PerryLayoutKey; 4] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_27),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_28),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_EXEC_NAMED_INDEX: [u32; 8] = [u32::MAX, u32::MAX, u32::MAX, 1, 2, 0, 3, u32::MAX];
/// A `RegExp.prototype.exec` result array's named layout (`ArrayHeader.named`):
/// `index`, `input`, `groups`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_EXEC_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x52540009,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 4,
    layout_id: crate::codegen_abi::native_layout_word("REGEX_EXEC_NAMED", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_EXEC_NAMED_KEYS.as_ptr(),
    key_count: 4,
    index_mask: 7,
    index: REGEX_EXEC_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_EXEC_NAMED);
static REGEX_EXEC_INDICES_NAMED_KEYS: [PerryLayoutKey; 5] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_27),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_28),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_30),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_EXEC_INDICES_NAMED_INDEX: [u32; 16] = [
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    2,
    3,
    u32::MAX,
    u32::MAX,
];
/// A `d`-flag exec result's named layout: `index`, `input`, `groups`, `indices`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_EXEC_INDICES_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x5254000A,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: crate::codegen_abi::native_layout_word("REGEX_EXEC_INDICES_NAMED", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_EXEC_INDICES_NAMED_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: REGEX_EXEC_INDICES_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_EXEC_INDICES_NAMED);
static REGEX_INDICES_GROUPS_NAMED_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_INDICES_GROUPS_NAMED_INDEX: [u32; 4] = [u32::MAX, 0, 1, u32::MAX];
/// A `d`-flag `indices` array's named layout: `groups`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_INDICES_GROUPS_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x5254000B,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: crate::codegen_abi::native_layout_word("REGEX_INDICES_GROUPS_NAMED", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_INDICES_GROUPS_NAMED_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: REGEX_INDICES_GROUPS_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_INDICES_GROUPS_NAMED);
static ARRAY_NAMED_KEYS: [PerryLayoutKey; 1] = [PerryLayoutKey {
    atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
    slot: 0,
    attrs: 0,
    _pad: [0; 3],
}];
static ARRAY_NAMED_INDEX: [u32; 2] = [u32::MAX, 0];
/// Empty array named family; absent iterator slot 0 is the hole marker.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ARRAY_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x5254000D,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 1,
    layout_id: crate::codegen_abi::native_layout_word("ARRAY_NAMED", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: ARRAY_NAMED_KEYS.as_ptr(),
    key_count: 1,
    index_mask: 1,
    index: ARRAY_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(ARRAY_NAMED);

// Fixed legacy SQLite column metadata; the five data properties are all present.
#[used(linker)]
#[export_name = "perry_image_literal_0_636f6c756d6e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_0: ImageString<7> = image(*b"column\0");
wasm_string_image!(SQLITE_IMAGE_0);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_636f6c756d6e"]
static SQLITE_ATOM_0: StaticAtom = StaticAtom {
    hash: atom_hash(b"column"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_0.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_0);
#[used(linker)]
#[export_name = "perry_image_literal_0_6461746162617365"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_1: ImageString<9> = image(*b"database\0");
wasm_string_image!(SQLITE_IMAGE_1);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_6461746162617365"]
static SQLITE_ATOM_1: StaticAtom = StaticAtom {
    hash: atom_hash(b"database"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_1.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_1);
#[used(linker)]
#[export_name = "perry_image_literal_0_6e616d65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_2: ImageString<5> = image(*b"name\0");
wasm_string_image!(SQLITE_IMAGE_2);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_6e616d65"]
static SQLITE_ATOM_2: StaticAtom = StaticAtom {
    hash: atom_hash(b"name"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_2.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_2);
#[used(linker)]
#[export_name = "perry_image_literal_0_7461626c65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_3: ImageString<6> = image(*b"table\0");
wasm_string_image!(SQLITE_IMAGE_3);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_7461626c65"]
static SQLITE_ATOM_3: StaticAtom = StaticAtom {
    hash: atom_hash(b"table"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_3.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_3);
#[used(linker)]
#[export_name = "perry_image_literal_0_74797065"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_4: ImageString<5> = image(*b"type\0");
wasm_string_image!(SQLITE_IMAGE_4);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_74797065"]
static SQLITE_ATOM_4: StaticAtom = StaticAtom {
    hash: atom_hash(b"type"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_4.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_4);
static SQLITE_COLUMN_KEYS: [PerryLayoutKey; 5] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_1),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_2),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_3),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_4),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
];
static SQLITE_COLUMN_INDEX: [u32; 16] = [
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    0,
    1,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    3,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub static mut SQLITE_COLUMN_METADATA: PerryLayoutDesc = PerryLayoutDesc {
    shape_id: 0,
    _layout_pad: 0,
    family: 0x53514c01,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: crate::codegen_abi::native_layout_word("SQLITE_COLUMN_METADATA", 0),
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: SQLITE_COLUMN_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: SQLITE_COLUMN_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(SQLITE_COLUMN_METADATA);

#[cfg(test)]
#[path = "runtime_layouts/slot_order_tests.rs"]
mod slot_order_tests;

#[cfg(test)]
#[path = "runtime_layouts/hole_tests.rs"]
mod hole_tests;

const fn runtime_layout_word(name: &str) -> u32 {
    const fn same(a: &str, b: &str) -> bool {
        let a = a.as_bytes();
        let b = b.as_bytes();
        if a.len() != b.len() {
            return false;
        }
        let mut i = 0;
        while i < a.len() {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    if same(name, "ARRAY_NAMED") {
        return crate::codegen_abi::native_layout_word("ARRAY_NAMED", 0);
    }
    if same(name, "ARRAY_PROTOTYPE_NAMED") {
        return crate::codegen_abi::native_layout_word("ARRAY_PROTOTYPE_NAMED", 0);
    }
    if same(name, "CJS_MODULE_RECORD") {
        return crate::codegen_abi::native_layout_word("CJS_MODULE_RECORD", 0);
    }
    if same(name, "DUPLEX") {
        return crate::codegen_abi::native_layout_word("DUPLEX", 1);
    }
    if same(name, "DUPLEX_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("DUPLEX_PROTOTYPE", 1);
    }
    if same(name, "EE_ASYNC_RESOURCE_PROTO") {
        return crate::codegen_abi::native_layout_word("EE_ASYNC_RESOURCE_PROTO", 0);
    }
    if same(name, "EVENT_EMITTER") {
        return crate::codegen_abi::native_layout_word("EVENT_EMITTER", 1);
    }
    if same(name, "EVENT_EMITTER_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("EVENT_EMITTER_PROTOTYPE", 0);
    }
    if same(name, "FUNCTION_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("FUNCTION_PROTOTYPE", 0);
    }
    if same(name, "IMMEDIATE") {
        return crate::codegen_abi::native_layout_word("IMMEDIATE", 1);
    }
    if same(name, "INCOMING_MESSAGE_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("INCOMING_MESSAGE_PROTOTYPE", 1);
    }
    if same(name, "ITERATOR_RESULT") {
        return crate::codegen_abi::native_layout_word("ITERATOR_RESULT", 0);
    }
    if same(name, "ITERATOR_RESULT_DONE_FIRST") {
        return crate::codegen_abi::native_layout_word("ITERATOR_RESULT_DONE_FIRST", 0);
    }
    if same(name, "LEGACY_STREAM") {
        return crate::codegen_abi::native_layout_word("LEGACY_STREAM", 1);
    }
    if same(name, "NATIVE_CTOR_DUPLEX") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_DUPLEX", 2);
    }
    if same(name, "NATIVE_CTOR_EVENT_EMITTER") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_EVENT_EMITTER", 2);
    }
    if same(name, "NATIVE_CTOR_INCOMING_MESSAGE") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_INCOMING_MESSAGE", 2);
    }
    if same(name, "NATIVE_CTOR_OUTGOING_MESSAGE") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_OUTGOING_MESSAGE", 2);
    }
    if same(name, "NATIVE_CTOR_PASSTHROUGH") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_PASSTHROUGH", 2);
    }
    if same(name, "NATIVE_CTOR_READABLE") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_READABLE", 2);
    }
    if same(name, "NATIVE_CTOR_SERVER_RESPONSE") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_SERVER_RESPONSE", 2);
    }
    if same(name, "NATIVE_CTOR_STREAM") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_STREAM", 2);
    }
    if same(name, "NATIVE_CTOR_TRANSFORM") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_TRANSFORM", 2);
    }
    if same(name, "NATIVE_CTOR_WRITABLE") {
        return crate::codegen_abi::native_layout_word("NATIVE_CTOR_WRITABLE", 2);
    }
    if same(name, "OBJECT_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("OBJECT_PROTOTYPE", 2);
    }
    if same(name, "OUTGOING_MESSAGE_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("OUTGOING_MESSAGE_PROTOTYPE", 1);
    }
    if same(name, "PASSTHROUGH") {
        return crate::codegen_abi::native_layout_word("PASSTHROUGH", 1);
    }
    if same(name, "PASSTHROUGH_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("PASSTHROUGH_PROTOTYPE", 1);
    }
    if same(name, "READABLE") {
        return crate::codegen_abi::native_layout_word("READABLE", 1);
    }
    if same(name, "READABLE_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("READABLE_PROTOTYPE", 1);
    }
    if same(name, "READABLE_STATE") {
        return crate::codegen_abi::native_layout_word("READABLE_STATE", 1);
    }
    if same(name, "READABLE_STATE_PROTO") {
        return crate::codegen_abi::native_layout_word("READABLE_STATE_PROTO", 0);
    }
    if same(name, "RECORD_F0_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F0_0", 0);
    }
    if same(name, "RECORD_F13_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F13_0", 0);
    }
    if same(name, "RECORD_F17_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F17_0", 0);
    }
    if same(name, "RECORD_F20_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F20_0", 0);
    }
    if same(name, "RECORD_F21_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F21_0", 0);
    }
    if same(name, "RECORD_F28_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F28_0", 0);
    }
    if same(name, "RECORD_F29_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F29_0", 0);
    }
    if same(name, "RECORD_F30_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F30_0", 0);
    }
    if same(name, "RECORD_F31_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F31_0", 0);
    }
    if same(name, "RECORD_F327_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F327_0", 0);
    }
    if same(name, "RECORD_F328_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F328_0", 0);
    }
    if same(name, "RECORD_F37_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F37_0", 0);
    }
    if same(name, "RECORD_F3_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F3_0", 0);
    }
    if same(name, "RECORD_F411_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F411_0", 0);
    }
    if same(name, "RECORD_F412_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F412_0", 0);
    }
    if same(name, "RECORD_F413_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F413_0", 0);
    }
    if same(name, "RECORD_F414_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F414_0", 0);
    }
    if same(name, "RECORD_F418_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F418_0", 0);
    }
    if same(name, "RECORD_F426_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F426_0", 0);
    }
    if same(name, "RECORD_F427_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F427_0", 0);
    }
    if same(name, "RECORD_F429_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F429_0", 0);
    }
    if same(name, "RECORD_F430_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F430_0", 0);
    }
    if same(name, "RECORD_F434_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F434_0", 0);
    }
    if same(name, "RECORD_F470_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F470_0", 0);
    }
    if same(name, "RECORD_F471_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F471_0", 0);
    }
    if same(name, "RECORD_F473_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F473_0", 0);
    }
    if same(name, "RECORD_F483_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F483_0", 0);
    }
    if same(name, "RECORD_F487_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F487_0", 0);
    }
    if same(name, "RECORD_F568_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F568_0", 0);
    }
    if same(name, "RECORD_F6_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F6_0", 0);
    }
    if same(name, "RECORD_F7_0") {
        return crate::codegen_abi::native_layout_word("RECORD_F7_0", 0);
    }
    if same(name, "RECORD_PCHECKED031") {
        return crate::codegen_abi::native_layout_word("RECORD_PCHECKED031", 0);
    }
    if same(name, "RECORD_Q435_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q435_0", 0);
    }
    if same(name, "RECORD_Q436_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q436_0", 0);
    }
    if same(name, "RECORD_Q439_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q439_0", 0);
    }
    if same(name, "RECORD_Q440_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q440_0", 0);
    }
    if same(name, "RECORD_Q441_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q441_0", 0);
    }
    if same(name, "RECORD_Q443_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q443_0", 0);
    }
    if same(name, "RECORD_Q448_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q448_0", 0);
    }
    if same(name, "RECORD_Q449_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q449_0", 0);
    }
    if same(name, "RECORD_Q450_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q450_0", 0);
    }
    if same(name, "RECORD_Q451_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q451_0", 0);
    }
    if same(name, "RECORD_Q452_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q452_0", 0);
    }
    if same(name, "RECORD_Q453_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q453_0", 0);
    }
    if same(name, "RECORD_Q454_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q454_0", 0);
    }
    if same(name, "RECORD_Q455_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q455_0", 0);
    }
    if same(name, "RECORD_Q456_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q456_0", 0);
    }
    if same(name, "RECORD_Q457_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q457_0", 0);
    }
    if same(name, "RECORD_Q458_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q458_0", 0);
    }
    if same(name, "RECORD_Q459_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q459_0", 0);
    }
    if same(name, "RECORD_Q460_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q460_0", 0);
    }
    if same(name, "RECORD_Q461_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q461_0", 0);
    }
    if same(name, "RECORD_Q462_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q462_0", 0);
    }
    if same(name, "RECORD_Q463_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q463_0", 0);
    }
    if same(name, "RECORD_Q464_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q464_0", 0);
    }
    if same(name, "RECORD_Q465_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q465_0", 0);
    }
    if same(name, "RECORD_Q466_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q466_0", 0);
    }
    if same(name, "RECORD_Q467_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q467_0", 0);
    }
    if same(name, "RECORD_Q468_0") {
        return crate::codegen_abi::native_layout_word("RECORD_Q468_0", 0);
    }
    if same(name, "RECORD_Q468_1") {
        return crate::codegen_abi::native_layout_word("RECORD_Q468_1", 0);
    }
    if same(name, "RECORD_Q468_2") {
        return crate::codegen_abi::native_layout_word("RECORD_Q468_2", 0);
    }
    if same(name, "RECORD_Q468_3") {
        return crate::codegen_abi::native_layout_word("RECORD_Q468_3", 0);
    }
    if same(name, "RECORD_U610") {
        return crate::codegen_abi::native_layout_word("RECORD_U610", 0);
    }
    if same(name, "RECORD_U613") {
        return crate::codegen_abi::native_layout_word("RECORD_U613", 0);
    }
    if same(name, "RECORD_V026") {
        return crate::codegen_abi::native_layout_word("RECORD_V026", 0);
    }
    if same(name, "REGEX_EXEC_INDICES_NAMED") {
        return crate::codegen_abi::native_layout_word("REGEX_EXEC_INDICES_NAMED", 0);
    }
    if same(name, "REGEX_EXEC_NAMED") {
        return crate::codegen_abi::native_layout_word("REGEX_EXEC_NAMED", 0);
    }
    if same(name, "REGEX_INDICES_GROUPS_NAMED") {
        return crate::codegen_abi::native_layout_word("REGEX_INDICES_GROUPS_NAMED", 0);
    }
    if same(name, "SERVER_RESPONSE") {
        return crate::codegen_abi::native_layout_word("SERVER_RESPONSE", 1);
    }
    if same(name, "SERVER_RESPONSE_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("SERVER_RESPONSE_PROTOTYPE", 1);
    }
    if same(name, "SPAWN_SYNC_RESULT") {
        return crate::codegen_abi::native_layout_word("SPAWN_SYNC_RESULT", 0);
    }
    if same(name, "SQLITE_COLUMN_METADATA") {
        return crate::codegen_abi::native_layout_word("SQLITE_COLUMN_METADATA", 0);
    }
    if same(name, "STREAM_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("STREAM_PROTOTYPE", 1);
    }
    if same(name, "TIMEOUT") {
        return crate::codegen_abi::native_layout_word("TIMEOUT", 1);
    }
    if same(name, "TRANSFORM") {
        return crate::codegen_abi::native_layout_word("TRANSFORM", 1);
    }
    if same(name, "TRANSFORM_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("TRANSFORM_PROTOTYPE", 1);
    }
    if same(name, "URL_COMPONENTS") {
        return crate::codegen_abi::native_layout_word("URL_COMPONENTS", 1);
    }
    if same(name, "URL_SEARCH_PARAMS") {
        return crate::codegen_abi::native_layout_word("URL_SEARCH_PARAMS", 1);
    }
    if same(name, "WRITABLE") {
        return crate::codegen_abi::native_layout_word("WRITABLE", 1);
    }
    if same(name, "WRITABLE_PROTOTYPE") {
        return crate::codegen_abi::native_layout_word("WRITABLE_PROTOTYPE", 1);
    }
    if same(name, "WRITABLE_STATE") {
        return crate::codegen_abi::native_layout_word("WRITABLE_STATE", 1);
    }
    if same(name, "WRITABLE_STATE_PROTO") {
        return crate::codegen_abi::native_layout_word("WRITABLE_STATE_PROTO", 0);
    }
    #[cfg(test)]
    if same(name, "HOLEY") {
        return 0x8000007d;
    }
    #[cfg(test)]
    if same(name, "HOLEY_PERMUTED") {
        return 0x8000007e;
    }
    #[cfg(test)]
    if same(name, "SOLID") {
        return 0x8000007f;
    }
    #[cfg(test)]
    if same(name, "PERMUTED") {
        return 0x80000080;
    }
    #[cfg(test)]
    if same(name, "POSITIONAL") {
        return 0x80000081;
    }
    #[cfg(test)]
    if same(name, "SYMBOL_PERMUTED") {
        return 0x80000082;
    }
    #[cfg(test)]
    if same(name, "PREFIX_MOVED") {
        return 0x80000083;
    }
    #[cfg(test)]
    if same(name, "LITERAL_OPTIONAL") {
        return 0x8000008a;
    }
    panic!("runtime descriptor absent from native catalog");
}

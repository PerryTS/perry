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
            family: $family, variant: $variant, kind: $kind, integrity: 0,
            slot_count: $name::SLOT_COUNT as u32,
            layout_id: crate::object::layout_desc::LAYOUT_ID_CARRIED,
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

#[used(linker)]
#[export_name = "perry_image_literal_0_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_0: ImageString<14> = image(*b"highWaterMark\0");
wasm_string_image!(IMAGE_0);

#[cfg(test)]
pub(crate) fn test_image_string() -> *const StringHeader {
    &IMAGE_0.string
}
#[used(linker)]
#[export_name = "perry_atom_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_0: StaticAtom = StaticAtom {
    hash: atom_hash(b"highWaterMark"),
    string: std::ptr::addr_of!(IMAGE_0.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_0);

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_1: ImageString<7> = image(*b"buffer\0");
wasm_string_image!(IMAGE_1);
#[used(linker)]
#[export_name = "perry_atom_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_1: StaticAtom = StaticAtom {
    hash: atom_hash(b"buffer"),
    string: std::ptr::addr_of!(IMAGE_1.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_1);

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_2: ImageString<12> = image(*b"bufferIndex\0");
wasm_string_image!(IMAGE_2);
#[used(linker)]
#[export_name = "perry_atom_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_2: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferIndex"),
    string: std::ptr::addr_of!(IMAGE_2.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_2);

#[used(linker)]
#[export_name = "perry_image_literal_0_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_3: ImageString<7> = image(*b"length\0");
wasm_string_image!(IMAGE_3);
#[used(linker)]
#[export_name = "perry_atom_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_3: StaticAtom = StaticAtom {
    hash: atom_hash(b"length"),
    string: std::ptr::addr_of!(IMAGE_3.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_3);

#[used(linker)]
#[export_name = "perry_image_literal_0_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_4: ImageString<6> = image(*b"pipes\0");
wasm_string_image!(IMAGE_4);
#[used(linker)]
#[export_name = "perry_atom_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_4: StaticAtom = StaticAtom {
    hash: atom_hash(b"pipes"),
    string: std::ptr::addr_of!(IMAGE_4.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_4);

#[used(linker)]
#[export_name = "perry_image_literal_0_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_5: ImageString<18> = image(*b"awaitDrainWriters\0");
wasm_string_image!(IMAGE_5);
#[used(linker)]
#[export_name = "perry_atom_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_5: StaticAtom = StaticAtom {
    hash: atom_hash(b"awaitDrainWriters"),
    string: std::ptr::addr_of!(IMAGE_5.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_5);

#[used(linker)]
#[export_name = "perry_image_literal_0_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_6: ImageString<7> = image(*b"corked\0");
wasm_string_image!(IMAGE_6);
#[used(linker)]
#[export_name = "perry_atom_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_6: StaticAtom = StaticAtom {
    hash: atom_hash(b"corked"),
    string: std::ptr::addr_of!(IMAGE_6.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_6);

#[used(linker)]
#[export_name = "perry_image_literal_0_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_7: ImageString<8> = image(*b"onwrite\0");
wasm_string_image!(IMAGE_7);
#[used(linker)]
#[export_name = "perry_atom_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_7: StaticAtom = StaticAtom {
    hash: atom_hash(b"onwrite"),
    string: std::ptr::addr_of!(IMAGE_7.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_7);

#[used(linker)]
#[export_name = "perry_image_literal_0_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_8: ImageString<9> = image(*b"writelen\0");
wasm_string_image!(IMAGE_8);
#[used(linker)]
#[export_name = "perry_atom_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_8: StaticAtom = StaticAtom {
    hash: atom_hash(b"writelen"),
    string: std::ptr::addr_of!(IMAGE_8.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_8);

#[used(linker)]
#[export_name = "perry_image_literal_0_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_9: ImageString<14> = image(*b"bufferedIndex\0");
wasm_string_image!(IMAGE_9);
#[used(linker)]
#[export_name = "perry_atom_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_9: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferedIndex"),
    string: std::ptr::addr_of!(IMAGE_9.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_9);

#[used(linker)]
#[export_name = "perry_image_literal_0_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_10: ImageString<10> = image(*b"pendingcb\0");
wasm_string_image!(IMAGE_10);
#[used(linker)]
#[export_name = "perry_atom_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_10: StaticAtom = StaticAtom {
    hash: atom_hash(b"pendingcb"),
    string: std::ptr::addr_of!(IMAGE_10.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_10);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d732d656e74726965733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_11: ImageString<35> = image(*b"#<perry:url-search-params-entries>\0");
wasm_string_image!(IMAGE_11);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d732d656e74726965733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_11: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params-entries>"),
    string: std::ptr::addr_of!(IMAGE_11.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_11);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d732d6f776e65723e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_12: ImageString<33> = image(*b"#<perry:url-search-params-owner>\0");
wasm_string_image!(IMAGE_12);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d732d6f776e65723e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_12: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params-owner>"),
    string: std::ptr::addr_of!(IMAGE_12.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_12);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_13: ImageString<18> = image(*b"#<perry:url-href>\0");
wasm_string_image!(IMAGE_13);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_13: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-href>"),
    string: std::ptr::addr_of!(IMAGE_13.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_13);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_14: ImageString<22> = image(*b"#<perry:url-protocol>\0");
wasm_string_image!(IMAGE_14);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_14: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-protocol>"),
    string: std::ptr::addr_of!(IMAGE_14.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_14);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_15: ImageString<18> = image(*b"#<perry:url-host>\0");
wasm_string_image!(IMAGE_15);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_15: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-host>"),
    string: std::ptr::addr_of!(IMAGE_15.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_15);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_16: ImageString<22> = image(*b"#<perry:url-hostname>\0");
wasm_string_image!(IMAGE_16);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_16: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hostname>"),
    string: std::ptr::addr_of!(IMAGE_16.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_16);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_17: ImageString<18> = image(*b"#<perry:url-port>\0");
wasm_string_image!(IMAGE_17);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_17: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-port>"),
    string: std::ptr::addr_of!(IMAGE_17.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_17);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_18: ImageString<22> = image(*b"#<perry:url-pathname>\0");
wasm_string_image!(IMAGE_18);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_18: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-pathname>"),
    string: std::ptr::addr_of!(IMAGE_18.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_18);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_19: ImageString<20> = image(*b"#<perry:url-search>\0");
wasm_string_image!(IMAGE_19);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_19: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search>"),
    string: std::ptr::addr_of!(IMAGE_19.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_19);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_20: ImageString<18> = image(*b"#<perry:url-hash>\0");
wasm_string_image!(IMAGE_20);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_20: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hash>"),
    string: std::ptr::addr_of!(IMAGE_20.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_20);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_21: ImageString<20> = image(*b"#<perry:url-origin>\0");
wasm_string_image!(IMAGE_21);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_21: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-origin>"),
    string: std::ptr::addr_of!(IMAGE_21.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_21);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_22: ImageString<27> = image(*b"#<perry:url-search-params>\0");
wasm_string_image!(IMAGE_22);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_22: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params>"),
    string: std::ptr::addr_of!(IMAGE_22.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_22);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_23: ImageString<22> = image(*b"#<perry:url-username>\0");
wasm_string_image!(IMAGE_23);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_23: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-username>"),
    string: std::ptr::addr_of!(IMAGE_23.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_23);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_24: ImageString<22> = image(*b"#<perry:url-password>\0");
wasm_string_image!(IMAGE_24);
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_24: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-password>"),
    string: std::ptr::addr_of!(IMAGE_24.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_24);

#[used(linker)]
#[export_name = "perry_image_literal_0_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_25: ImageString<6> = image(*b"value\0");
wasm_string_image!(IMAGE_25);
#[used(linker)]
#[export_name = "perry_atom_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_25: StaticAtom = StaticAtom {
    hash: atom_hash(b"value"),
    string: std::ptr::addr_of!(IMAGE_25.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_25);

#[used(linker)]
#[export_name = "perry_image_literal_0_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_26: ImageString<5> = image(*b"done\0");
wasm_string_image!(IMAGE_26);
#[used(linker)]
#[export_name = "perry_atom_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_26: StaticAtom = StaticAtom {
    hash: atom_hash(b"done"),
    string: std::ptr::addr_of!(IMAGE_26.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_26);

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
    family: 0x52540001,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 6,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540002,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 7,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540003,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540004,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 12,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540005,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540006,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540007,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x52540008,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
#[used(linker)]
#[export_name = "perry_image_literal_0_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_27: ImageString<6> = image(*b"index\0");
wasm_string_image!(IMAGE_27);
#[used(linker)]
#[export_name = "perry_atom_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_27: StaticAtom = StaticAtom {
    hash: atom_hash(b"index"),
    string: std::ptr::addr_of!(IMAGE_27.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_27);
#[used(linker)]
#[export_name = "perry_image_literal_0_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_28: ImageString<6> = image(*b"input\0");
wasm_string_image!(IMAGE_28);
#[used(linker)]
#[export_name = "perry_atom_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_28: StaticAtom = StaticAtom {
    hash: atom_hash(b"input"),
    string: std::ptr::addr_of!(IMAGE_28.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_28);
#[used(linker)]
#[export_name = "perry_image_literal_0_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_29: ImageString<7> = image(*b"groups\0");
wasm_string_image!(IMAGE_29);
#[used(linker)]
#[export_name = "perry_atom_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_29: StaticAtom = StaticAtom {
    hash: atom_hash(b"groups"),
    string: std::ptr::addr_of!(IMAGE_29.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_29);
#[used(linker)]
#[export_name = "perry_image_literal_0_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_30: ImageString<8> = image(*b"indices\0");
wasm_string_image!(IMAGE_30);
#[used(linker)]
#[export_name = "perry_atom_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_30: StaticAtom = StaticAtom {
    hash: atom_hash(b"indices"),
    string: std::ptr::addr_of!(IMAGE_30.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_30);
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
    family: 0x52540009,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 4,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x5254000A,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x5254000B,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x5254000D,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 1,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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
    family: 0x53514c01,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
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

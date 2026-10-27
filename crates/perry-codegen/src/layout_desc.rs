//! Object model step 0 (OBJECT_MODEL_2026-10-13.md §6a): emit the module's
//! layout descriptors and static atoms.
//!
//! Every object-literal expression with static keys is a family (§3.2); its
//! layout is its key sequence, slot = position. One `PerryLayoutDesc` per
//! family goes into the section `perry_layouts`, one `StaticAtom` per key
//! spelling into `perry_atoms` (weak_odr, named by spelling, so the linker
//! keeps one per program and it points at the one image string of that
//! spelling). The runtime (`perry-runtime/src/object/layout_desc.rs`)
//! registers them at startup; layout ids are assigned there, in section
//! order, and written into `desc.layout_id`, which generated code loads.
//!
//! The record types below mirror the runtime's `#[repr(C)]` structs, whose
//! sizes and offsets the runtime pins with const asserts.

pub(crate) mod array_families;
pub(crate) mod create_families;
pub(crate) mod function_families;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use crate::layout_families::{FamilyKey, FamilyLayout as ClassFamilyLayout, ProgramLayouts};
use perry_hir::{Expr, Module as HirModule, Stmt};

use crate::module::LlModule;
use crate::strings::StringPool;

/// `%PerryLayoutDesc`: family, variant, kind, integrity, slot_count,
/// template word, rep, proto, keys, key_count, index_mask, index, class, parent,
/// methods, identity and pad (80 bytes LP64; 64 bytes ILP32).
pub const DESC_TY: &str =
    "{ i32, i16, i8, i8, i32, i32, i64, ptr, ptr, i32, i32, ptr, i32, i32, ptr, i32, i32 }";
mod methods;
/// `%PerryLayoutKey`: atom, slot, attrs, pad (16 bytes).
pub const KEY_TY: &str = "{ ptr, i32, i8, [3 x i8] }";
/// `%StaticAtom`: hash, string, kind, pad, index (24 bytes; §6a).
pub const ATOM_TY: &str = "{ i64, ptr, i8, [3 x i8], i32 }";
// Windows atom records share one read-only COFF image with the Rust atoms
// and boundary markers. A writable input would be split into another section.
fn atom_storage(target: &str) -> &'static str {
    if target.contains("windows") {
        "constant"
    } else {
        "global"
    }
}

/// Byte offset of the immutable template LayoutWord in a descriptor.
pub const DESC_LAYOUT_ID_OFFSET: u32 = 12;

/// `PerryLayoutDesc.layout_id` of a layout objects carry (the runtime's
/// `LAYOUT_ID_CARRIED`): registered at startup. Every other descriptor is
/// static data and is emitted with 0.
const LAYOUT_ID_CARRIED: u32 = u32::MAX;

/// Object model step 1 (§3.2): a literal family's first key added after
/// birth. The family is born with a slot for it (`slot`, after the birth
/// keys), and a store through a receiver codegen ties to the family writes
/// that slot and the layout word of the family state that holds it.
///
/// Coexistence: the shape table takes presence from a shape's key list, so
/// "the key is present" is a layout of its own (`desc_global`, registered at
/// startup) until holes in slots are the presence authority (steps 4 and 5).
#[derive(Clone, Debug)]
pub(crate) struct FamilyAdd {
    pub key: String,
    pub slot: u32,
    /// The descriptor of the state "birth keys + `key`".
    pub desc_global: String,
    /// The family's birth key count and birth rep.
    pub birth_keys: u32,
    pub birth_rep: u64,
}

/// One carried family state to emit: its keys in slot order.
#[derive(Clone, Debug)]
pub(crate) struct FamilyLayout {
    pub desc_global: String,
    pub keys: Vec<String>,
    pub slot_count: u32,
    pub rep: u64,
    pub array_named: bool,
}

#[derive(Default)]
struct ModuleFamilies {
    adds: HashMap<String, FamilyAdd>,
    layouts: Vec<FamilyLayout>,
    /// Reads of a family key that are Numbers by construction (expression
    /// addresses of this module's HIR): present on an exact family object,
    /// and every store of the key on that local writes a Number.
    numeric_reads: std::collections::HashSet<usize>,
    /// The module prefix of its array layout descriptors, when it emits them.
    array_prefix: Option<String>,
}

thread_local! {
    /// The module being compiled on this thread (codegen of one module runs
    /// on one thread, as `static_constfn`'s module tables do).
    static MODULE_FAMILIES: RefCell<ModuleFamilies> = RefCell::new(ModuleFamilies::default());
}

/// Install the module's literal families (`codegen/mod.rs`), replacing the
/// previous module's.
pub(crate) fn set_module_families(adds: HashMap<String, FamilyAdd>, layouts: Vec<FamilyLayout>) {
    MODULE_FAMILIES.with(|m| {
        *m.borrow_mut() = ModuleFamilies {
            adds,
            layouts,
            numeric_reads: Default::default(),
            array_prefix: None,
        }
    });
}

/// The module emits array layouts, named with `prefix`.
pub(crate) fn set_array_layout_prefix(prefix: &str) {
    MODULE_FAMILIES.with(|m| m.borrow_mut().array_prefix = Some(prefix.to_owned()));
}

/// The descriptor symbol of a module's array layout `ordinal`.
pub(crate) fn array_layout_symbol(prefix: &str, ordinal: u32) -> String {
    format!("perry_array_layout_{prefix}_{ordinal}")
}

/// The descriptor of array layout `ordinal` when this module emits array
/// layouts.
pub(crate) fn array_layout_desc(ordinal: u32) -> Option<String> {
    MODULE_FAMILIES.with(|m| {
        m.borrow()
            .array_prefix
            .as_deref()
            .map(|prefix| array_layout_symbol(prefix, ordinal))
    })
}

/// Record reads of a family key proven Numbers (`collectors/ptr_shape.rs`).
pub(crate) fn note_family_numeric_reads(reads: impl IntoIterator<Item = usize>) {
    MODULE_FAMILIES.with(|m| m.borrow_mut().numeric_reads.extend(reads));
}

/// Whether the read at `addr` is a family key proven a Number.
pub(crate) fn family_numeric_read(addr: usize) -> bool {
    MODULE_FAMILIES.with(|m| m.borrow().numeric_reads.contains(&addr))
}

/// The family add of class `class_name` when `key` is it.
pub(crate) fn family_add(class_name: &str, key: &str) -> Option<FamilyAdd> {
    MODULE_FAMILIES.with(|m| {
        m.borrow()
            .adds
            .get(class_name)
            .filter(|add| add.key == key)
            .cloned()
    })
}

thread_local! {
    /// Case 2c: the class family descriptors this module's code allocates
    /// from (`lower_call/new_alloc.rs`); [`emit_module_layouts`] declares the
    /// ones another module defines.
    static FAMILY_DESC_USES: RefCell<std::collections::BTreeSet<(u32, Option<usize>)>> =
        RefCell::new(std::collections::BTreeSet::new());
}

/// The descriptor global of class `class_id`'s family layout, noted as used
/// by this module.
pub(crate) fn note_family_desc_use(class_id: u32) -> String {
    note_family_variant_desc_use(class_id, None)
}

/// The default or creation-variant family descriptor, including a declaration
/// when this module only reads instances created in another module.
pub(crate) fn note_family_variant_desc_use(class_id: u32, variant: Option<usize>) -> String {
    FAMILY_DESC_USES.with(|u| u.borrow_mut().insert((class_id, variant)));
    match variant {
        Some(v) => format!("@perry_layout_class_{class_id}_v{v}"),
        None => format!("@perry_layout_class_{class_id}"),
    }
}

/// The i32 alias of a carried descriptor's `layout_id` word.
pub(crate) fn layout_id_global(desc_global: &str) -> String {
    format!("{desc_global}_id")
}

/// `(birth key count, birth rep)` of class `class_name`'s literal family.
pub(crate) fn family_birth_rep(class_name: &str) -> Option<(u32, u64)> {
    MODULE_FAMILIES.with(|m| {
        m.borrow()
            .adds
            .get(class_name)
            .map(|add| (add.birth_keys, add.birth_rep))
    })
}

/// Whether `key` spells an atom (the descriptor emitter's test).
pub(crate) fn key_is_atom(key: &str) -> bool {
    !key.is_empty() && key.len() <= ATOM_MAX_BYTES
}

/// Longest key spelling that is an atom (the runtime's `INTERN_MAX_BYTE_LEN`).
const ATOM_MAX_BYTES: usize = 64;

/// The section of a record the runtime finds by walking its section (atoms,
/// layout descriptors, the shadow-name descriptor), not by a reference from
/// code. Many of those records have no reference at all: the entry module's
/// coloured-key atoms, the atoms and descriptors of literal families emitted
/// as static data. On ELF the `__start_`/`__stop_` bounds the runtime names
/// keep the whole section through `--gc-sections`; Mach-O has no such rule,
/// and perry links with `-dead_strip`, which dropped every unreferenced record
/// while the runtime's own (`#[used]`) records stayed. The runtime then never
/// seeded those atoms or colours: the program's literal keys got a second,
/// heap atom, and a coloured read of the image atom answered absent (zod's
/// `this.refinement.bind(this)` threw "Bind must be called on a function" on
/// macOS). `no_dead_strip` on the section keeps each record in it.
fn section(triple: &str, elf: &str, mach: &str, win: &str) -> String {
    if triple.contains("apple") {
        format!("{mach},regular,no_dead_strip")
    } else if triple.contains("windows") {
        win.to_string()
    } else {
        elf.to_string()
    }
}

/// The hash of key `key`'s atom (the runtime's property-atom hash), the
/// unknown-receiver probe's operand.
pub(crate) fn atom_hash(key: &str) -> u64 {
    fnv1a(key.as_bytes())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

thread_local! {
    /// This compile's key colouring (`emit_module_layouts`), read by [`atom`].
    static COLOURS: std::cell::RefCell<BTreeMap<String, (u32, u32)>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The colour bits of an image string with text `bytes` (perry-runtime
/// `string::atom_colour`): bits 16..24 the mask bit + 1, bits 24..32 the
/// colour slot; 0 for an uncoloured text or a colour that does not fit.
pub(crate) fn image_colour_flags(bytes: &[u8]) -> u32 {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return 0;
    };
    match COLOURS.with(|c| c.borrow().get(text).copied()) {
        Some((bit, slot)) if bit < 127 && slot < 256 => ((bit + 1) << 16) | (slot << 24),
        _ => 0,
    }
}

/// The static atom global of `key` (emitting it once per module).
pub(crate) fn atom(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    emitted: &mut BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    let colour = COLOURS.with(|c| c.borrow().get(key).copied());
    if let Some(name) = emitted.get(key) {
        return Some(name.clone());
    }
    let idx = strings.intern(key);
    let entry = strings.entry(idx);
    if entry.is_wtf8 || entry.byte_len == 0 {
        return None;
    }
    let image = crate::codegen::string_pool_image_pointer(entry);
    let spelling: String = key.bytes().map(|b| format!("{b:02x}")).collect();
    let name = format!("perry_atom_{spelling}");
    let sect = section(
        &llmod.target_triple,
        "perry_atoms",
        "__DATA,__perry_atoms",
        ".perryatm$m",
    );
    let atom_storage = atom_storage(&llmod.target_triple);
    llmod.add_coalesced_global(&name, format!(
        "@{name} = weak_odr {atom_storage} {ATOM_TY} {{ i64 {}, ptr {image}, i8 {}, [3 x i8] [i8 {}, i8 0, i8 0], i32 {} }}, section \"{sect}\", align 8",
        fnv1a(key.as_bytes()) as i64,
        crate::runtime_abi::ATOM_KIND_STRING,
        u8::from(colour.is_some()),
        // Object model step 5 (case 2c): a coloured key's (mask bit, slot),
        // the same in every module (one program-wide colouring), so the
        // weak_odr records coalesce to one definition.
        colour.map_or(0, |(bit, slot)| bit | (slot << 8)) as i32
    ));
    emitted.insert(key.to_string(), name.clone());
    Some(name)
}

/// The static atom global of a symbol key of a carried prototype layout
/// (emitting it once per module) and its index hash: a well-known symbol
/// (`ATOM_KIND_WELL_KNOWN_SYMBOL`, its number) or a registered one
/// (`ATOM_KIND_REGISTERED_SYMBOL`, its registry key as an image string), or
/// a module-local one (`ATOM_KIND_BINDING_SYMBOL`, this module's own record:
/// the prototype's bind hands its symbol).
fn symbol_atom(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    emitted: &mut BTreeMap<String, String>,
    module_prefix: &str,
    key: &create_families::ImageKey,
) -> (String, u64) {
    if let create_families::ImageKey::Local(binding) = key {
        let spelling = format!("@@local:{module_prefix}:{binding}");
        let hash = fnv1a(spelling.as_bytes());
        let spelling = format!("\u{1}{spelling}");
        if let Some(name) = emitted.get(&spelling) {
            return (name.clone(), hash);
        }
        let name = format!("perry_atom_local_{module_prefix}_{binding}");
        let sect = section(
            &llmod.target_triple,
            "perry_atoms",
            "__DATA,__perry_atoms",
            ".perryatm$m",
        );
        let atom_storage = atom_storage(&llmod.target_triple);
        llmod.add_raw_global(format!(
            "@{name} = internal {atom_storage} {ATOM_TY} {{ i64 {}, ptr null, i8 {}, [3 x i8] zeroinitializer, i32 0 }}, section \"{sect}\", align 8",
            hash as i64,
            crate::runtime_abi::ATOM_KIND_BINDING_SYMBOL,
        ));
        emitted.insert(spelling, name.clone());
        return (name, hash);
    }
    let (spelling, kind, image, index) = match key {
        create_families::ImageKey::WellKnown(i) => (
            format!("@@wk:{i}"),
            crate::runtime_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
            "null".to_string(),
            *i,
        ),
        create_families::ImageKey::Registered(desc) => {
            let idx = strings.intern(desc);
            let image = crate::codegen::string_pool_image_pointer(strings.entry(idx));
            (
                format!("@@for:{desc}"),
                crate::runtime_abi::ATOM_KIND_REGISTERED_SYMBOL,
                image,
                0,
            )
        }
        create_families::ImageKey::Str(k) => unreachable!("string key {k} is not a symbol atom"),
        create_families::ImageKey::Local(_) => unreachable!("handled above"),
    };
    let hash = fnv1a(spelling.as_bytes());
    // Not a string key's spelling: string atoms share `emitted`.
    let spelling = format!("\u{1}{spelling}");
    if let Some(name) = emitted.get(&spelling) {
        return (name.clone(), hash);
    }
    let hex: String = spelling.bytes().map(|b| format!("{b:02x}")).collect();
    let name = format!("perry_atom_sym_{hex}");
    let sect = section(
        &llmod.target_triple,
        "perry_atoms",
        "__DATA,__perry_atoms",
        ".perryatm$m",
    );
    let atom_storage = atom_storage(&llmod.target_triple);
    llmod.add_coalesced_global(&name, format!(
        "@{name} = weak_odr {atom_storage} {ATOM_TY} {{ i64 {}, ptr {image}, i8 {kind}, [3 x i8] zeroinitializer, i32 {index} }}, section \"{sect}\", align 8",
        hash as i64,
    ));
    emitted.insert(spelling, name.clone());
    (name, hash)
}

/// Emit a carried prototype layout's descriptor: its keys in order (slot =
/// position) with their attribute entries, its key index, and its
/// [[Prototype]] word.
fn emit_image_descriptor(
    llmod: &mut LlModule,
    sect: &str,
    desc_name: &str,
    keys: &[(String, u64, u8)],
    family: u32,
    proto: &str,
    variant: u16,
    integrity: u8,
) {
    let count = keys.len();
    let cap = (count * 2).next_power_of_two().max(2);
    let mut index = vec![u32::MAX; cap];
    for (pos, (_, hash, _)) in keys.iter().enumerate() {
        let mut at = (*hash as usize) & (cap - 1);
        while index[at] != u32::MAX {
            at = (at + 1) & (cap - 1);
        }
        index[at] = pos as u32;
    }
    let key_items: Vec<String> = keys
        .iter()
        .enumerate()
        .map(|(i, (a, _, attrs))| {
            format!(
                "{KEY_TY} {{ ptr @{a}, i32 {i}, i8 {}, [3 x i8] zeroinitializer }}",
                *attrs as i8
            )
        })
        .collect();
    let keys_name = format!("perry_layout_keys_{desc_name}");
    let index_name = format!("perry_layout_index_{desc_name}");
    llmod.add_raw_global(format!(
        "@{keys_name} = private unnamed_addr constant [{count} x {KEY_TY}] [{}], align 8",
        key_items.join(", ")
    ));
    let index_items: Vec<String> = index.iter().map(|v| format!("i32 {}", *v as i32)).collect();
    llmod.add_raw_global(format!(
        "@{index_name} = private unnamed_addr constant [{cap} x i32] [{}], align 4",
        index_items.join(", ")
    ));
    let identity = 0;
    let word = crate::layout_ordinals::descriptor_word(
        desc_name,
        format!("{keys:?}|{family}|{proto}|{variant}|{integrity}"),
        if proto == "1" {
            2
        } else if proto == "0" {
            0
        } else {
            1
        },
        false,
        true,
        identity,
    );
    llmod.add_raw_global(format!(
        "@{desc_name} = global {DESC_TY} {{ i32 {family}, i16 {variant}, i8 0, i8 {integrity}, i32 {}, i32 {}, i64 0, ptr inttoptr (i64 {proto} to ptr), ptr @{keys_name}, i32 {count}, i32 {}, ptr @{index_name}, i32 0, i32 0, ptr null, i32 {identity}, i32 0 }}, section \"{sect}\", align 8",
        count.max(1),
        word as i32,
        cap - 1
    ));
}

/// A bounded spelling fingerprint: length and its first six bytes. Full
/// byte equality resolves every collision; it is not a name's identity.
fn shadow_name_fingerprint(bytes: &[u8]) -> u64 {
    let mut key = (bytes.len() as u64) << 48;
    for (i, byte) in bytes.iter().take(6).enumerate() {
        key |= u64::from(*byte) << (8 * i);
    }
    key & !1
}

fn shadow_name_bucket(key: u64, mask: usize) -> usize {
    (((key ^ (key >> 32)).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 32) as usize) & mask
}

/// The program's immutable shadow-name descriptor. Its spelling index and
/// admission masks are emitted by the entry module, never learned at run time.
/// Bit 0 of each fingerprint marks receiver names.
pub(crate) fn emit_shadow_names(llmod: &mut LlModule, strings: &mut StringPool, output_type: &str) {
    if output_type == "dylib" || output_type == "staticlib" || llmod.target_triple.contains("wasm")
    {
        return;
    }
    let mut names = crate::proto_constants::shadow_names();
    if names.is_empty() {
        return;
    }
    names.sort_by_key(|(name, _)| shadow_name_fingerprint(name.as_bytes()));
    let index_len = (2 * names.len()).next_power_of_two();
    let mut index = vec![u32::MAX; index_len];
    let mut masks = [[0u64; 64]; 2];
    let mut records = Vec::with_capacity(names.len());
    for (position, (name, receiver)) in names.iter().enumerate() {
        let bytes = name.as_bytes();
        let row = bytes.len() & 63;
        let prefix = u64::from(bytes.first().copied().unwrap_or(0)) * 33
            ^ u64::from(bytes.get(1).copied().unwrap_or(0));
        let bit = 1u64 << (prefix & 63);
        masks[1][row] |= bit;
        if *receiver {
            masks[0][row] |= bit;
        }
        let idx = strings.intern(name);
        let image = crate::codegen::string_pool_image_pointer(strings.entry(idx));
        let fingerprint = shadow_name_fingerprint(bytes);
        records.push(format!(
            "{ATOM_TY} {{ i64 {}, ptr {image}, i8 0, [3 x i8] zeroinitializer, i32 0 }}",
            (fingerprint | u64::from(*receiver)) as i64
        ));
        let mut bucket = shadow_name_bucket(fingerprint, index_len - 1);
        while index[bucket] != u32::MAX {
            bucket = (bucket + 1) & (index_len - 1);
        }
        index[bucket] = position as u32;
    }
    let count = names.len();
    llmod.add_raw_global(format!(
        "@perry_shadow_name_records = private constant [{count} x {ATOM_TY}] [{}], align 8",
        records.join(", ")
    ));
    let index_words = index
        .iter()
        .map(|w| format!("i32 {}", *w as i32))
        .collect::<Vec<_>>()
        .join(", ");
    llmod.add_raw_global(format!(
        "@perry_shadow_name_index = private constant [{index_len} x i32] [{index_words}], align 8"
    ));
    let masks = masks
        .iter()
        .flatten()
        .map(|w| format!("i64 {}", *w as i64))
        .collect::<Vec<_>>()
        .join(", ");
    llmod.add_raw_global(format!(
        "@perry_shadow_name_data = private constant {{ [128 x i64], ptr, ptr, i64 }} {{ [128 x i64] [{masks}], ptr @perry_shadow_name_index, ptr @perry_shadow_name_records, i64 {} }}, align 8", index_len - 1
    ));
    let sect = section(
        &llmod.target_triple,
        "perry_shadow_names",
        "__DATA,__perry_shnames",
        ".perryshn$m",
    );
    llmod.add_raw_global(format!(
        "@perry_shadow_name_descriptor = constant {ATOM_TY} {{ i64 {count}, ptr @perry_shadow_name_data, i8 0, [3 x i8] zeroinitializer, i32 0 }}, section \"{sect}\", align 8"
    ));
}

/// The static key list of a literal family, or `None` when it is not one.
fn literal_keys(props: &[(String, Expr)]) -> Option<Vec<String>> {
    let mut keys: Vec<String> = Vec::new();
    for (k, _) in props {
        if k == "__proto__" {
            return None;
        }
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    (!keys.is_empty()).then_some(keys)
}

fn walk_expr<'a>(e: &'a Expr, out: &mut Vec<&'a [(String, Expr)]>) {
    match e {
        Expr::Object(props) => out.push(props),
        Expr::Closure { body, params, .. } => {
            for p in params {
                if let Some(d) = &p.default {
                    walk_expr(d, out);
                }
            }
            walk_stmts(body, out);
            return;
        }
        _ => {}
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| walk_expr(c, out));
}

fn walk_stmts<'a>(body: &'a [Stmt], out: &mut Vec<&'a [(String, Expr)]>) {
    for s in body {
        match s {
            Stmt::Let { init: Some(e), .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e)) => walk_expr(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                walk_expr(condition, out);
                walk_stmts(then_branch, out);
                if let Some(b) = else_branch {
                    walk_stmts(b, out);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                walk_expr(condition, out);
                walk_stmts(body, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    walk_stmts(std::slice::from_ref(i.as_ref()), out);
                }
                if let Some(c) = condition {
                    walk_expr(c, out);
                }
                if let Some(u) = update {
                    walk_expr(u, out);
                }
                walk_stmts(body, out);
            }
            Stmt::Labeled { body, .. } => walk_stmts(std::slice::from_ref(body.as_ref()), out),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                walk_stmts(body, out);
                if let Some(c) = catch {
                    walk_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    walk_stmts(f, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                walk_expr(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        walk_expr(t, out);
                    }
                    walk_stmts(&c.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Every static-key object literal of the module, in a deterministic order.
fn module_literals(hir: &HirModule) -> Vec<Vec<String>> {
    let mut lits: Vec<&[(String, Expr)]> = Vec::new();
    walk_stmts(&hir.init, &mut lits);
    for f in &hir.functions {
        walk_stmts(&f.body, &mut lits);
    }
    for c in &hir.classes {
        for f in c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
        {
            walk_stmts(&f.body, &mut lits);
        }
        for (_, f) in c.getters.iter().chain(c.setters.iter()) {
            walk_stmts(&f.body, &mut lits);
        }
    }
    for g in &hir.globals {
        if let Some(e) = &g.init {
            walk_expr(e, &mut lits);
        }
    }
    let mut out: Vec<Vec<String>> = Vec::new();
    // Closed-shape literals lower to `new __AnonShape_<hash>()`: one family
    // per key sequence, its keys the synthesized class's fields in order.
    for c in &hir.classes {
        if c.name.starts_with("__AnonShape_") {
            let mut keys: Vec<String> = Vec::new();
            for f in &c.fields {
                if f.name == "__proto__" {
                    keys.clear();
                    break;
                }
                if !keys.contains(&f.name) {
                    keys.push(f.name.clone());
                }
            }
            if !keys.is_empty() {
                out.push(keys);
            }
        }
    }
    out.extend(lits.into_iter().filter_map(literal_keys));
    out
}

/// Install the program's key colouring on this thread, which [`atom`] reads:
/// the static seed unit emits atoms outside any module's codegen.
pub(crate) fn set_colours(layouts: &ProgramLayouts) {
    COLOURS.with(|c| *c.borrow_mut() = layouts.colored.clone());
}

/// Whether this compile emits layout descriptors at all (executables only:
/// atoms point at image strings, which only executables carry).
/// `PERRY_LAYOUT_DESCS=0` disables emission and with it every family.
pub(crate) fn layouts_enabled(output_type: &str, target_triple: &str) -> bool {
    output_type != "dylib"
        && output_type != "staticlib"
        && !target_triple.contains("wasm")
        && std::env::var("PERRY_LAYOUT_DESCS").as_deref() != Ok("0")
}

/// The descriptor prototype tag of a class family: 0 (the class's own
/// prototype) for every class but a declared-keys record, whose instances
/// have a null prototype (1, `require.cache`) or `%Object.prototype%` (3,
/// `exports`).
fn declared_proto_tag(class: &perry_hir::Class) -> u8 {
    if !class.is_declared_keys_record() {
        0
    } else if class.declared_keys_null_proto() {
        1
    } else {
        3
    }
}

/// One descriptor plus its keys and index tables. `keys` is the key sequence
/// in enumeration order with each key's slot.
#[allow(clippy::too_many_arguments)]
fn emit_desc(
    llmod: &mut LlModule,
    sect: &str,
    names: (&str, &str, &str),
    family: u32,
    keys: &[(FamilyKey, String, u32)],
    slot_count: u32,
    class: (u32, u32),
    methods: &str,
    proto: u8,
) {
    let (keys_name, index_name, desc_name) = names;
    let count = keys.len();
    let cap = (count * 2).next_power_of_two().max(2);
    let mut index = vec![u32::MAX; cap];
    // The index answers property lookups (`perry-runtime`
    // `layout_desc::desc_position_hashed`): a private element is no property,
    // so it has no index entry.
    for (pos, (k, _, _)) in keys.iter().enumerate() {
        let Some(k) = k.name() else {
            continue;
        };
        let mut at = (fnv1a(k.as_bytes()) as usize) & (cap - 1);
        while index[at] != u32::MAX {
            at = (at + 1) & (cap - 1);
        }
        index[at] = pos as u32;
    }
    // Every key of a class family (case 2c) carries
    // `perry_abi::LAYOUT_KEY_OPTIONAL` (0x80): absent while its slot holds
    // the hole. A birth writes no key; a store adds it, and a private
    // element's claim fills it (`LAYOUT_KEY_PRIVATE_FIELD` too).
    let key_items: Vec<String> = keys
        .iter()
        .map(|(k, a, slot)| {
            let attrs = crate::runtime_abi::LAYOUT_KEY_OPTIONAL
                | if k.is_private() {
                    crate::runtime_abi::LAYOUT_KEY_PRIVATE_FIELD
                } else {
                    0
                };
            format!(
                "{KEY_TY} {{ ptr @{a}, i32 {slot}, i8 {}, [3 x i8] zeroinitializer }}",
                attrs as i8
            )
        })
        .collect();
    llmod.add_raw_global(format!(
        "@{keys_name} = private unnamed_addr constant [{count} x {KEY_TY}] [{}], align 8",
        key_items.join(", ")
    ));
    let index_items: Vec<String> = index.iter().map(|v| format!("i32 {}", *v as i32)).collect();
    llmod.add_raw_global(format!(
        "@{index_name} = private unnamed_addr constant [{cap} x i32] [{}], align 4",
        index_items.join(", ")
    ));
    let word = crate::layout_ordinals::descriptor_word(
        desc_name,
        format!("{keys:?}|{family}|{slot_count}|{class:?}"),
        0,
        true,
        // Both nominal and admitted classless families carry their DPA word.
        true,
        0,
    );
    llmod.add_raw_global(format!(
        "@{desc_name} = global {DESC_TY} {{ i32 {family}, i16 0, i8 0, i8 0, i32 {slot_count}, i32 {}, i64 0, ptr inttoptr (i64 {} to ptr), ptr @{keys_name}, i32 {count}, i32 {}, ptr @{index_name}, i32 {}, i32 {}, ptr {methods}, i32 0, i32 0 }}, section \"{sect}\", align 8",
        word as i32,
        // The `PerryLayoutDesc.proto` tag (perry-runtime `layout_desc`).
        proto,
        cap - 1,
        class.0,
        class.1
    ));
}

/// Emit the module's layout descriptors (executables only: atoms point at
/// image strings, which only executables carry). Returns how many.
pub(crate) fn emit_module_layouts(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    hir: &HirModule,
    module_prefix: &str,
    output_type: &str,
    layouts: &ProgramLayouts,
    is_entry_module: bool,
    namespace: &[crate::NamespaceEntry],
    adopts_cjs_exports: bool,
) -> usize {
    if !layouts_enabled(output_type, &llmod.target_triple) {
        return 0;
    }
    COLOURS.with(|c| *c.borrow_mut() = layouts.colored.clone());
    let mut atoms: BTreeMap<String, String> = BTreeMap::new();
    // Step 5 (case 2c): the entry module carries an atom for every coloured
    // key, so the runtime learns every colour from the atom section.
    if is_entry_module {
        for key in layouts.colored.keys() {
            let _ = atom(llmod, strings, &mut atoms, key);
        }
    }
    let sect = section(
        &llmod.target_triple,
        "perry_layouts",
        "__DATA,__perry_layouts",
        ".perrylay$m",
    );
    let mut n = 0usize;
    // Step 1: the carried family states first (generated code references
    // them by name), then every literal family as static data.
    let carried: Vec<FamilyLayout> = MODULE_FAMILIES.with(|m| m.borrow().layouts.clone());

    for (ordinal, fam) in carried.iter().enumerate() {
        let mut atom_names = Vec::with_capacity(fam.keys.len());
        for k in &fam.keys {
            atom_names.push(
                atom(llmod, strings, &mut atoms, k)
                    .expect("a family key is an atom (`key_is_atom`)"),
            );
        }
        let mut keys = fam.keys.clone();
        let first_hash = if fam.array_named {
            let name = "perry_atom_well_known_iterator";
            let atom_sect = section(
                &llmod.target_triple,
                "perry_atoms",
                "__DATA,__perry_atoms",
                ".perryatm$m",
            );
            let hash = crate::runtime_abi::well_known_symbol_atom_hash(
                crate::runtime_abi::WELL_KNOWN_SYMBOL_ITERATOR,
            );
            // weak_odr coalesces one well-known atom across modules.
            if !atoms.contains_key(name) {
                let atom_storage = atom_storage(&llmod.target_triple);
                llmod.add_coalesced_global(name, format!(
                    "@{name} = weak_odr {atom_storage} {ATOM_TY} {{ i64 {}, ptr null, i8 {}, [3 x i8] zeroinitializer, i32 {} }}, section \"{atom_sect}\", align 8",
                    hash as i64, crate::runtime_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
                    crate::runtime_abi::WELL_KNOWN_SYMBOL_ITERATOR));
                atoms.insert(name.into(), name.into());
            }
            keys.insert(0, String::new());
            atom_names.insert(0, name.into());
            Some(hash)
        } else {
            None
        };
        let family = (fnv1a(format!("{}#fam{ordinal}", hir.name).as_bytes()) as u32) & 0x7FFF_FFFF;
        emit_descriptor(
            llmod,
            &sect,
            &fam.desc_global,
            &format!("{}_fam", fam.desc_global),
            &keys,
            &atom_names,
            family,
            fam.slot_count + u32::from(fam.array_named),
            fam.rep,
            LAYOUT_ID_CARRIED,
            first_hash,
            "0",
        );
        // The id word on its own, so a function loads it once at entry.
        llmod.add_raw_global(format!(
            "@{} = internal alias i32, getelementptr (i8, ptr @{}, i64 {DESC_LAYOUT_ID_OFFSET})",
            layout_id_global(&fam.desc_global),
            fam.desc_global
        ));
        n += 1;
    }
    // Step 1: `Object.create(P)` families (`create_families`): each
    // prototype binding's identity record, then each creation point's
    // carried layout naming it.
    let (protos, create_sites, images) = create_families::emission();
    for global in &protos {
        llmod.add_raw_global(format!("@{global} = internal global i64 0, align 8"));
    }
    // Object model §3.6: each prototype's carried layout (its birth when
    // the object its initializer built is exactly this).
    for image in &images {
        let mut keys = Vec::with_capacity(image.entries.len());
        for e in &image.entries {
            let (name, hash) = match &e.key {
                create_families::ImageKey::Str(k) => (
                    atom(llmod, strings, &mut atoms, k)
                        .expect("an image key is an atom (`create_families::layout_key`)"),
                    fnv1a(k.as_bytes()),
                ),
                key => symbol_atom(llmod, strings, &mut atoms, module_prefix, key),
            };
            keys.push((name, hash, e.attrs));
        }
        let family = (fnv1a(image.desc_global.as_bytes()) as u32) & 0x7FFF_FFFF;
        let proto = match &image.proto {
            None => "0".to_string(),
            Some(None) => "1".to_string(),
            Some(Some(fam)) => format!("ptrtoint (ptr @{fam} to i64)"),
        };
        emit_image_descriptor(
            llmod,
            &sect,
            &image.desc_global,
            &keys,
            family,
            &proto,
            crate::runtime_abi::LAYOUT_VARIANT_PROTOTYPE_IMAGE,
            0,
        );
        llmod.add_raw_global(format!(
            "@{} = internal alias i32, getelementptr (i8, ptr @{}, i64 {DESC_LAYOUT_ID_OFFSET})",
            layout_id_global(&image.desc_global),
            image.desc_global
        ));
        n += 1;
    }
    for site in &create_sites {
        let atom_names: Vec<String> = site
            .keys
            .iter()
            .map(|k| {
                atom(llmod, strings, &mut atoms, k)
                    .expect("a create-family key is an atom (`create_families::layout_key`)")
            })
            .collect();
        let family = (fnv1a(site.desc_global.as_bytes()) as u32) & 0x7FFF_FFFF;
        emit_descriptor(
            llmod,
            &sect,
            &site.desc_global,
            &format!("{}_fam", site.desc_global),
            &site.keys,
            &atom_names,
            family,
            site.slots.max(1),
            0,
            LAYOUT_ID_CARRIED,
            None,
            &format!("ptrtoint (ptr @{} to i64)", site.proto_global),
        );
        llmod.add_raw_global(format!(
            "@{} = internal alias i32, getelementptr (i8, ptr @{}, i64 {DESC_LAYOUT_ID_OFFSET})",
            layout_id_global(&site.desc_global),
            site.desc_global
        ));
        n += 1;
    }
    'family: for (ordinal, keys) in module_literals(hir).into_iter().enumerate() {
        let mut atom_names = Vec::with_capacity(keys.len());
        for k in &keys {
            match atom(llmod, strings, &mut atoms, k) {
                Some(a) => atom_names.push(a),
                None => continue 'family,
            }
        }
        let family = (fnv1a(format!("{}#{ordinal}", hir.name).as_bytes()) as u32) & 0x7FFF_FFFF;
        let desc_name = format!("perry_layout_{module_prefix}_{n}");
        let count = keys.len() as u32;
        emit_descriptor(
            llmod,
            &sect,
            &desc_name,
            &format!("{module_prefix}_{n}"),
            &keys,
            &atom_names,
            family,
            count,
            0,
            0,
            None,
            "0",
        );
        n += 1;
    }
    // Completed literal facts were emitted by the compiler and installed by
    // the static seed before descriptor registration. This descriptor names
    // that same shape, including its ConstFn slots; allocation never matches
    // values or discovers a different shape.
    for (shape, id) in crate::codegen::static_constfn::literal_seed_descriptors() {
        if shape.proto != crate::codegen::BirthProto::Literal {
            continue;
        }
        let keys: Vec<String> = shape
            .keys
            .split(|&b| b == 0)
            .take(shape.key_count as usize)
            .map(|k| String::from_utf8(k.to_vec()).expect("literal key UTF-8"))
            .collect();
        let atom_names: Vec<_> = keys
            .iter()
            .map(|key| atom(llmod, strings, &mut atoms, key).expect("static literal atom"))
            .collect();
        let name = literal_desc_symbol(module_prefix, id);
        let family = (fnv1a(name.as_bytes()) as u32) & 0x7FFF_FFFF;
        emit_descriptor(
            llmod,
            &sect,
            &name,
            &name,
            &keys,
            &atom_names,
            family,
            shape.live,
            shape.rep,
            id,
            None,
            "0",
        );
        n += 1;
    }
    // Case 2c: the family descriptors this module allocates from but another
    // module defines.
    let local: std::collections::BTreeSet<u32> = hir
        .classes
        .iter()
        .filter(|c| layouts.families.contains_key(&c.id))
        .map(|c| c.id)
        .collect();
    for (cid, variant) in FAMILY_DESC_USES.with(|u| std::mem::take(&mut *u.borrow_mut())) {
        if !local.contains(&cid) {
            let suffix = variant.map(|v| format!("_v{v}")).unwrap_or_default();
            llmod.add_raw_global(format!(
                "@perry_layout_class_{cid}{suffix} = external global {DESC_TY}"
            ));
        }
    }
    // Case 2c: one descriptor per static class family defined here, with the
    // slots `layout_families::assign_slots` gave its keys.
    'class: for class in &hir.classes {
        let Some(fam): Option<&ClassFamilyLayout> = layouts.families.get(&class.id) else {
            continue;
        };
        let mut named = Vec::with_capacity(fam.keys.len());
        for (k, slot) in &fam.keys {
            match atom(llmod, strings, &mut atoms, k.spelling()) {
                Some(a) => named.push((k.clone(), a, *slot)),
                None => continue 'class,
            }
        }
        // Sabotage for the slot witness (`perry_family_slot_verify`): the
        // descriptor disagrees with the slots compiled code uses.
        if named.len() >= 2 && std::env::var("PERRY_FAMILY_SLOT_SABOTAGE").as_deref() == Ok("1") {
            let s0 = named[0].2;
            named[0].2 = named[1].2;
            named[1].2 = s0;
        }
        let family =
            (fnv1a(format!("{}#class#{}", hir.name, class.name).as_bytes()) as u32) & 0x7FFF_FFFF;
        // The creation split's layouts first (all or none: the table names
        // every one).
        let mut variants: Vec<(u64, Vec<(FamilyKey, String, u32)>, u32)> = Vec::new();
        for v in &fam.variants {
            let mut vnamed = Vec::with_capacity(v.keys.len());
            for (k, slot) in &v.keys {
                match atom(llmod, strings, &mut atoms, k.spelling()) {
                    Some(a) => vnamed.push((k.clone(), a, *slot)),
                    None => break,
                }
            }
            if vnamed.len() != v.keys.len() {
                variants.clear();
                break;
            }
            variants.push((v.key_bits, vnamed, v.slot_count));
        }
        let methods = methods::emit(llmod, strings, &mut atoms, fam);
        let literal = layouts.literal_families.contains(&fam.class_id);
        let identity = if literal {
            (0, 0)
        } else {
            (fam.class_id, fam.parent_class_id)
        };
        // First-birth literal descriptors are static image data outside the
        // startup enumeration. DPA assigns their immutable row words; the
        // runtime publishes the current agent's facts only at first birth.
        let family_section = if literal {
            section(
                &llmod.target_triple,
                "perry_literal_layouts",
                "__DATA,__perry_litlay",
                ".perrylit$m",
            )
        } else {
            sect.clone()
        };
        emit_desc(
            llmod,
            &family_section,
            (
                &format!("perry_layout_keys_{module_prefix}_{n}"),
                &format!("perry_layout_index_{module_prefix}_{n}"),
                &format!("perry_layout_class_{}", fam.class_id),
            ),
            family,
            &named,
            fam.slot_count,
            identity,
            &methods,
            declared_proto_tag(class),
        );
        n += 1;
        let mut entries = Vec::with_capacity(variants.len());
        for (j, (key_bits, vnamed, slot_count)) in variants.iter().enumerate() {
            let name = format!("perry_layout_class_{}_v{j}", fam.class_id);
            emit_desc(
                llmod,
                &family_section,
                (
                    &format!("perry_layout_keys_{module_prefix}_{n}"),
                    &format!("perry_layout_index_{module_prefix}_{n}"),
                    &name,
                ),
                family,
                vnamed,
                *slot_count,
                identity,
                &methods,
                declared_proto_tag(class),
            );
            n += 1;
            entries.push((*key_bits, name));
        }
        // `perry-runtime` `layout_desc::PerryFamilyVariants`: the argument
        // index, the count, then (constant bits, descriptor) sorted by the
        // constant's bits.
        if !entries.is_empty() {
            entries.sort();
            let items: Vec<String> = entries
                .iter()
                .map(|(k, name)| format!("{{ i64, ptr }} {{ i64 {}, ptr @{name} }}", *k as i64))
                .collect();
            llmod.add_raw_global(format!(
                "@perry_family_variants_{} = constant {{ i32, i32, [{} x {{ i64, ptr }}] }} {{ i32 {}, i32 {}, [{} x {{ i64, ptr }}] [{}] }}, align 8",
                fam.class_id,
                entries.len(),
                fam.discriminant,
                entries.len(),
                entries.len(),
                items.join(", ")
            ));
        }
    }
    // The module's namespace object (ECMA-262 10.4.6): one carried layout,
    // born complete (`js_create_namespace_layout`).
    let triple = llmod.target_triple.clone();
    if let Some(ns_keys) = namespace_layout(
        namespace,
        module_prefix,
        output_type,
        &triple,
        adopts_cjs_exports,
    ) {
        let mut keys: Vec<(String, u64, u8)> = ns_keys
            .iter()
            .map(|(k, attrs)| {
                let name = atom(llmod, strings, &mut atoms, k)
                    .expect("a namespace layout key is an atom (`namespace_layout`)");
                (name, fnv1a(k.as_bytes()), *attrs)
            })
            .collect();
        // 10.4.6: `@@toStringTag` is an own non-writable, non-enumerable,
        // non-configurable data property holding "Module", after the names.
        let (tag, tag_hash) = symbol_atom(
            llmod,
            strings,
            &mut atoms,
            module_prefix,
            &create_families::ImageKey::WellKnown(NAMESPACE_TAG_SYMBOL),
        );
        keys.push((tag, tag_hash, NAMESPACE_TAG_ATTRS));
        let desc = namespace_desc_symbol(module_prefix);
        let family = (fnv1a(desc.as_bytes()) as u32) & 0x7FFF_FFFF;
        emit_image_descriptor(
            llmod,
            &sect,
            &desc,
            &keys,
            family,
            "1",
            crate::runtime_abi::LAYOUT_VARIANT_NAMESPACE,
            crate::runtime_abi::LAYOUT_INTEGRITY_NON_EXTENSIBLE,
        );
        n += 1;
    }
    // Append missing seeded atoms after existing layout atoms. This retains
    // the startup ordering of unrelated keys while covering spilled contents.
    for key in crate::codegen::static_shape_ids::module_seed_keys() {
        atom(llmod, strings, &mut atoms, &key);
    }

    function_families::emit(llmod, strings, &mut atoms);
    n
}

/// The well-known symbol number of `Symbol.toStringTag`.
const NAMESPACE_TAG_SYMBOL: u32 = 4;
const _: () =
    assert!(crate::runtime_abi::WELL_KNOWN_SYMBOLS[NAMESPACE_TAG_SYMBOL as usize].len() == 11);
/// A namespace's `@@toStringTag` attributes: non-writable, non-enumerable,
/// non-configurable data.
const NAMESPACE_TAG_ATTRS: u8 = crate::runtime_abi::LAYOUT_KEY_NON_WRITABLE
    | crate::runtime_abi::LAYOUT_KEY_NON_ENUMERABLE
    | crate::runtime_abi::LAYOUT_KEY_NON_CONFIGURABLE;
/// The value of a namespace's `@@toStringTag` (ECMA-262 10.4.6).
pub(crate) const NAMESPACE_TAG: &str = "Module";

/// The descriptor of module `prefix`'s namespace layout.
pub(crate) fn namespace_desc_symbol(prefix: &str) -> String {
    format!("perry_layout_ns_{prefix}")
}

/// Whether namespace entry `entry` of module `module_prefix` is a live
/// binding: its key is an accessor whose getter reads the binding at access
/// time. A variable's value can change after the namespace is built (and may
/// be in its TDZ), and `export * as Self from "./self"` names this module's
/// own namespace, which is stored only after it is built (#10160).
pub(crate) fn namespace_entry_is_live(entry: &crate::NamespaceEntry, module_prefix: &str) -> bool {
    match &entry.kind {
        crate::NamespaceEntryKind::LocalVar { .. }
        | crate::NamespaceEntryKind::ForeignVar { .. } => true,
        crate::NamespaceEntryKind::NestedNamespace { source_prefix } => {
            source_prefix == module_prefix
        }
        _ => false,
    }
}

/// A module namespace object's compiler-owned layout (ECMA-262 10.4.6): its
/// export names in entry order (the driver sorts them; slot = position),
/// each with its attribute entry, followed in the descriptor by
/// `@@toStringTag` ([`NAMESPACE_TAG`]). A live binding is a non-configurable
/// enumerable accessor with a getter, every other export a writable,
/// enumerable, non-configurable data property. The descriptor also says
/// `null` [[Prototype]] and non-extensible.
///
/// `None` when this compile emits no layouts, a name repeats, or a name is
/// no atom: then the namespace is built key by key. The populator and the
/// descriptor emitter both ask this one function.
pub(crate) fn namespace_layout(
    entries: &[crate::NamespaceEntry],
    module_prefix: &str,
    output_type: &str,
    target_triple: &str,
    adopts_cjs_exports: bool,
) -> Option<Vec<(String, u8)>> {
    use crate::runtime_abi::{
        LAYOUT_KEY_ACCESSOR, LAYOUT_KEY_HAS_GET, LAYOUT_KEY_NON_CONFIGURABLE,
        LAYOUT_KEY_NON_WRITABLE,
    };
    // A CommonJS namespace under Bun interop takes its run-time
    // `module.exports` keys after birth (`js_create_cjs_namespace`): its key
    // set is not fixed, so it has no layout.
    if adopts_cjs_exports || !layouts_enabled(output_type, target_triple) {
        return None;
    }
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        if !key_is_atom(&entry.name) || !seen.insert(entry.name.as_str()) {
            return None;
        }
        let attrs = if namespace_entry_is_live(entry, module_prefix) {
            LAYOUT_KEY_ACCESSOR
                | LAYOUT_KEY_HAS_GET
                | LAYOUT_KEY_NON_WRITABLE
                | LAYOUT_KEY_NON_CONFIGURABLE
        } else {
            LAYOUT_KEY_NON_CONFIGURABLE
        };
        keys.push((entry.name.clone(), attrs));
    }
    Some(keys)
}

/// Descriptor of a compiler-declared completed literal shape.
pub(crate) fn literal_desc_symbol(prefix: &str, id: u32) -> String {
    format!("perry_layout_literal_{prefix}_{id}")
}

/// Emit one descriptor (its key sequence, slot = position, and key index).
#[allow(clippy::too_many_arguments)]
fn emit_descriptor(
    llmod: &mut LlModule,
    sect: &str,
    desc_name: &str,
    suffix: &str,
    keys: &[String],
    atom_names: &[String],
    family: u32,
    slot_count: u32,
    rep: u64,
    layout_id: u32,
    first_hash: Option<u64>,
    proto: &str,
) {
    {
        let count = keys.len();
        let cap = (count * 2).next_power_of_two().max(2);
        let mut index = vec![u32::MAX; cap];
        for (pos, k) in keys.iter().enumerate() {
            let hash = if pos == 0 {
                first_hash.unwrap_or_else(|| fnv1a(k.as_bytes()))
            } else {
                fnv1a(k.as_bytes())
            };
            let mut at = (hash as usize) & (cap - 1);
            while index[at] != u32::MAX {
                at = (at + 1) & (cap - 1);
            }
            index[at] = pos as u32;
        }
        let keys_name = format!("perry_layout_keys_{suffix}");
        let index_name = format!("perry_layout_index_{suffix}");
        let key_items: Vec<String> = atom_names
            .iter()
            .enumerate()
            .map(|(i, a)| {
                format!("{KEY_TY} {{ ptr @{a}, i32 {i}, i8 0, [3 x i8] zeroinitializer }}")
            })
            .collect();
        llmod.add_raw_global(format!(
            "@{keys_name} = private unnamed_addr constant [{count} x {KEY_TY}] [{}], align 8",
            key_items.join(", ")
        ));
        let index_items: Vec<String> = index.iter().map(|v| format!("i32 {}", *v as i32)).collect();
        llmod.add_raw_global(format!(
            "@{index_name} = private unnamed_addr constant [{cap} x i32] [{}], align 4",
            index_items.join(", ")
        ));
        let identity = if layout_id == LAYOUT_ID_CARRIED {
            0
        } else {
            layout_id
        };
        let word = crate::layout_ordinals::descriptor_word(
            desc_name,
            format!("{keys:?}|{family}|{slot_count}|{rep}|{first_hash:?}|{proto}"),
            if proto == "1" {
                2
            } else if proto == "0" {
                0
            } else {
                1
            },
            false,
            layout_id != 0,
            identity,
        );
        llmod.add_raw_global(format!(
            "@{desc_name} = global {DESC_TY} {{ i32 {family}, i16 0, i8 0, i8 0, i32 {slot_count}, i32 {}, i64 {}, ptr inttoptr (i64 {proto} to ptr), ptr @{keys_name}, i32 {count}, i32 {}, ptr @{index_name}, i32 0, i32 0, ptr null, i32 {identity}, i32 0 }}, section \"{sect}\", align 8",
            word as i32,
            rep as i64,
            cap - 1
        ));
    }
}

#[cfg(test)]
mod comdat_tests;

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

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use perry_hir::{Expr, Module as HirModule, Stmt};

use crate::module::LlModule;
use crate::strings::StringPool;

/// `%PerryLayoutDesc`: family, variant, kind, integrity, slot_count,
/// layout_id, rep, proto, keys, key_count, index_mask, index (56 bytes).
pub const DESC_TY: &str = "{ i32, i16, i8, i8, i32, i32, i64, i64, ptr, i32, i32, ptr }";
/// `%PerryLayoutKey`: atom, slot, attrs, pad (16 bytes).
pub const KEY_TY: &str = "{ ptr, i32, i8, [3 x i8] }";
/// `%StaticAtom`: hash, string (16 bytes).
pub const ATOM_TY: &str = "{ i64, ptr }";
/// Byte offset of `layout_id` in a descriptor.
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
}

#[derive(Default)]
struct ModuleFamilies {
    adds: HashMap<String, FamilyAdd>,
    layouts: Vec<FamilyLayout>,
    /// Reads of a family key that are Numbers by construction (expression
    /// addresses of this module's HIR): present on an exact family object,
    /// and every store of the key on that local writes a Number.
    numeric_reads: std::collections::HashSet<usize>,
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
        }
    });
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

fn section(triple: &str, elf: &str, mach: &str, win: &str) -> String {
    if triple.contains("apple") {
        mach.to_string()
    } else if triple.contains("windows") {
        win.to_string()
    } else {
        elf.to_string()
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The static atom global of `key` (emitting it once per module).
fn atom(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    emitted: &mut BTreeMap<String, String>,
    key: &str,
) -> Option<String> {
    if let Some(name) = emitted.get(key) {
        return Some(name.clone());
    }
    let idx = strings.intern(key);
    let entry = strings.entry(idx);
    if entry.is_wtf8 || entry.byte_len == 0 || entry.byte_len > ATOM_MAX_BYTES {
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
    llmod.add_raw_global(format!(
        "@{name} = weak_odr global {ATOM_TY} {{ i64 {}, ptr {image} }}, section \"{sect}\", align 8",
        fnv1a(key.as_bytes()) as i64
    ));
    emitted.insert(key.to_string(), name.clone());
    Some(name)
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

/// Whether this compile emits layout descriptors at all (executables only:
/// atoms point at image strings, which only executables carry).
/// `PERRY_LAYOUT_DESCS=0` disables emission and with it every family.
pub(crate) fn layouts_enabled(output_type: &str, target_triple: &str) -> bool {
    output_type != "dylib"
        && output_type != "staticlib"
        && !target_triple.contains("wasm")
        && std::env::var("PERRY_LAYOUT_DESCS").as_deref() != Ok("0")
}

/// Emit the module's layout descriptors (executables only: atoms point at
/// image strings, which only executables carry). Returns how many.
pub(crate) fn emit_module_layouts(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    hir: &HirModule,
    module_prefix: &str,
    output_type: &str,
) -> usize {
    if !layouts_enabled(output_type, &llmod.target_triple) {
        return 0;
    }
    let mut atoms: BTreeMap<String, String> = BTreeMap::new();
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
        let family = (fnv1a(format!("{}#fam{ordinal}", hir.name).as_bytes()) as u32) & 0x7FFF_FFFF;
        emit_descriptor(
            llmod,
            &sect,
            &fam.desc_global,
            &format!("{}_fam", fam.desc_global),
            &fam.keys,
            &atom_names,
            family,
            fam.slot_count,
            fam.rep,
            LAYOUT_ID_CARRIED,
        );
        // The id word on its own, so a function loads it once at entry.
        llmod.add_raw_global(format!(
            "@{} = internal alias i32, getelementptr (i8, ptr @{}, i64 {DESC_LAYOUT_ID_OFFSET})",
            layout_id_global(&fam.desc_global),
            fam.desc_global
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
        );
        n += 1;
    }
    n
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
) {
    {
        let count = keys.len();
        let cap = (count * 2).next_power_of_two().max(2);
        let mut index = vec![u32::MAX; cap];
        for (pos, k) in keys.iter().enumerate() {
            let mut at = (fnv1a(k.as_bytes()) as usize) & (cap - 1);
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
        llmod.add_raw_global(format!(
            "@{desc_name} = global {DESC_TY} {{ i32 {family}, i16 0, i8 0, i8 0, i32 {slot_count}, i32 {}, i64 {}, i64 0, ptr @{keys_name}, i32 {count}, i32 {}, ptr @{index_name} }}, section \"{sect}\", align 8",
            layout_id as i32,
            rep as i64,
            cap - 1
        ));
    }
}

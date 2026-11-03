//! Constant record trees retain their ordinary HIR shapes, but construction is
//! data-driven. A descriptor references the same rooted keys and typed ShapeId
//! globals as `new`; it never changes the representation used by property reads.
//!
//! A tree may also hold fresh closures (an object literal of arrow getters):
//! each is a closure node naming a birth record (`crate::closure_births`), and
//! the runtime bears it in tree order from the caller's capture words.

use std::collections::HashMap;

use perry_hir::Expr;

use super::FnCtx;
use crate::types::{LlvmType, DOUBLE, I32, I64, PTR};

const MIN_NODES: usize = 256;
const MAX_DEPTH: usize = 128;
// Must match runtime::array::literal_descriptor::LiteralShape (repr(C)).
const SHAPE_TYPE: &str = "{ i32, i32, ptr, ptr, ptr, i32, ptr, i32, i32, i64 }";

/// One `LiteralShape` record. `keys` and `shape_id` name the class's keys and
/// ShapeId slot globals; the masks are constants (or `null`).
struct ShapeRecord {
    class_id: u32,
    field_count: usize,
    keys: String,
    shape_id: String,
    raw_mask: String,
    raw_mask_len: usize,
    pointer_mask: String,
    pointer_mask_len: usize,
    width: u32,
    rep: u64,
}

impl ShapeRecord {
    fn constant(&self) -> String {
        format!(
            "{SHAPE_TYPE} {{ i32 {}, i32 {}, ptr @{}, ptr @{}, ptr {}, i32 {}, ptr {}, i32 {}, i32 {}, i64 {} }}",
            self.class_id,
            self.field_count,
            self.keys,
            self.shape_id,
            self.raw_mask,
            self.raw_mask_len,
            self.pointer_mask,
            self.pointer_mask_len,
            self.width,
            self.rep,
        )
    }
}

#[derive(Default)]
struct Descriptor {
    bytes: Vec<u8>,
    shapes: Vec<ShapeRecord>,
    shape_indices: HashMap<String, u32>,
    nodes: usize,
    closures: Vec<crate::closure_births::PlainBirth>,
}

impl Descriptor {
    fn u32(&mut self, value: usize) -> Option<()> {
        self.bytes
            .extend_from_slice(&u32::try_from(value).ok()?.to_le_bytes());
        Some(())
    }

    fn shape(&mut self, ctx: &FnCtx<'_>, name: &str, argc: usize) -> Option<u32> {
        if let Some(index) = self.shape_indices.get(name) {
            return Some(*index);
        }
        let class = ctx.classes.get(name)?;
        if !class.is_literal_shape() || argc != class.fields.len() {
            return None;
        }
        let keys = ctx.class_keys_globals.get(name)?;
        let class_id = ctx.class_ids.get(name)?;
        if ctx.class_field_counts.get(name).copied()? as usize != argc {
            return None;
        }
        let layout = crate::typed_shape::class_typed_layout(ctx.classes, name);
        let mask_ref = |words: &[u64], global: String| {
            if words.is_empty() {
                "null".to_string()
            } else {
                format!("@{global}")
            }
        };
        let raw_mask = mask_ref(
            &layout.raw_f64_mask_words,
            crate::typed_shape::raw_f64_mask_global_name_from_keys_global(keys),
        );
        let pointer_mask = mask_ref(
            &layout.pointer_mask_words,
            crate::typed_shape::mask_global_name_from_keys_global(keys),
        );
        let shape_id = crate::typed_shape::shape_id_global_name_from_keys_global(keys);
        // The birth rep the module-init mint gave that id (T1).
        let rep = ctx.class_birth_reps.get(keys).copied().unwrap_or(0);
        let index = u32::try_from(self.shapes.len()).ok()?;
        let width = argc as u32
            + crate::lower_call::new_alloc::birth_slack_in(
                class,
                &|name| ctx.classes.get(name).copied(),
                ctx.anon_key_adds,
            );
        self.shapes.push(ShapeRecord {
            class_id: *class_id,
            field_count: argc,
            keys: keys.clone(),
            shape_id,
            raw_mask,
            raw_mask_len: layout.raw_f64_mask_words.len(),
            pointer_mask,
            pointer_mask_len: layout.pointer_mask_words.len(),
            width,
            rep,
        });
        self.shape_indices.insert(name.to_string(), index);
        Some(index)
    }

    fn value(&mut self, ctx: &FnCtx<'_>, expr: &Expr, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.nodes += 1;
        match expr {
            Expr::Number(n) => {
                self.bytes.push(0);
                self.bytes.extend_from_slice(&n.to_le_bytes());
            }
            Expr::Integer(n) => {
                self.bytes.push(0);
                self.bytes.extend_from_slice(&(*n as f64).to_le_bytes());
            }
            Expr::Array(elements, _) => {
                self.bytes.push(1);
                self.u32(elements.len())?;
                for element in elements {
                    self.value(ctx, element, depth + 1)?;
                }
            }
            Expr::Bool(true) => self.bytes.push(2),
            Expr::Bool(false) => self.bytes.push(3),
            Expr::Null => self.bytes.push(4),
            Expr::Undefined => self.bytes.push(5),
            Expr::String(value) => {
                self.bytes.push(6);
                self.u32(value.len())?;
                self.bytes.extend_from_slice(value.as_bytes());
            }
            Expr::New {
                class_name,
                args,
                cap_args_appended: 0,
                ..
            } => {
                // Recheck arity on cache hits too: malformed/extra-argument
                // constructions must retain normal evaluation and call semantics.
                if ctx.classes.get(class_name)?.fields.len() != args.len() {
                    return None;
                }
                let index = self.shape(ctx, class_name, args.len())?;
                self.bytes.push(7);
                self.bytes.extend_from_slice(&index.to_le_bytes());
                for arg in args {
                    self.value(ctx, arg, depth + 1)?;
                }
            }
            Expr::Closure { .. } => {
                let birth = crate::closure_births::plain_birth(ctx, expr)?;
                self.bytes.push(8);
                self.u32(self.closures.len())?;
                self.closures.push(birth);
            }
            // Calls, spreads, computed properties, captures and user classes
            // all keep ordinary evaluation; no partially serialized expression
            // is ever emitted or evaluated.
            _ => return None,
        }
        Some(())
    }
}

pub(super) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> anyhow::Result<Option<String>> {
    if let Expr::EnumImage {
        class_name,
        stores,
        strict,
    } = expr
    {
        return lower_enum_image(ctx, class_name, stores, *strict).map(Some);
    }
    if !matches!(expr, Expr::Array(_, _) | Expr::New { .. }) {
        return Ok(None);
    }
    if ctx.program_has_worker && crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return Ok(None);
    }
    let mut descriptor = Descriptor::default();
    if descriptor.value(ctx, expr, 0).is_none() {
        return Ok(None);
    }
    // A closure node replaces a whole inline birth, so a run of them pays for
    // the table on its own.
    let enough = descriptor.nodes >= MIN_NODES
        || descriptor.closures.len() >= crate::closure_births::MIN_RUN;
    if !enough || descriptor.shapes.is_empty() {
        return Ok(None);
    }
    let Ok(len) = u32::try_from(descriptor.bytes.len()) else {
        return Ok(None);
    };
    let index = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let function: String = ctx
        .func
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let name = format!("perry_literal_{function}_{index}");
    let mut bytes = String::with_capacity(descriptor.bytes.len() * 3);
    for byte in &descriptor.bytes {
        use std::fmt::Write;
        write!(&mut bytes, "\\{byte:02X}").unwrap();
    }
    ctx.typed_parse_rodata.push(format!(
        "@{name} = private unnamed_addr constant [{len} x i8] c\"{bytes}\"",
    ));
    let count = descriptor.shapes.len();
    let bytes_ref = format!("@{name}");
    let shapes_ref = if ctx.program_has_worker {
        frame_shapes(ctx, &descriptor)
    } else {
        let records: Vec<String> = descriptor
            .shapes
            .iter()
            .map(ShapeRecord::constant)
            .collect();
        ctx.typed_parse_rodata.push(format!(
            "@{name}_shapes = private constant [{count} x {SHAPE_TYPE}] [{}]",
            records.join(", "),
        ));
        format!("@{name}_shapes")
    };
    let len = len.to_string();
    let count = count.to_string();
    let mut args = vec![
        (PTR, bytes_ref.as_str()),
        (I32, len.as_str()),
        (PTR, shapes_ref.as_str()),
        (I32, count.as_str()),
    ];
    if descriptor.closures.is_empty() {
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_value_from_literal_descriptor",
            &args,
        )));
    }
    let mut births = crate::closure_births::Births::default();
    for birth in &descriptor.closures {
        births.add(ctx, birth, None);
    }
    let (operands, words) = births.emit(ctx)?;
    args.extend(operands.iter().map(|(ty, value)| (*ty, value.as_str())));
    let value = ctx
        .block()
        .call(DOUBLE, "js_value_from_literal_descriptor_closures", &args);
    words.release(ctx);
    Ok(Some(value))
}

/// A fresh enum object (`perry_hir::lower::enum_image`): its literal layout
/// with each key's last value, as a one-shape descriptor, plus the stores
/// themselves in source order (`u32` count, then per store the key as
/// `u32` length and bytes and the value as a descriptor node). The runtime
/// births the object from the layout when `%Object.prototype%` is pristine
/// and replays the stores (`[[Set]]`, in the IIFE's mode) otherwise. A program whose shape table
/// cannot be built (ILP32 with workers) sends no shape: always the replay.
fn lower_enum_image(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    stores: &[(String, perry_hir::EnumValue)],
    strict: bool,
) -> anyhow::Result<String> {
    let value_expr = |value: &perry_hir::EnumValue| match value {
        perry_hir::EnumValue::Number(n) => Expr::Number(*n),
        perry_hir::EnumValue::String(s) => Expr::String(s.clone()),
        perry_hir::EnumValue::Computed => unreachable!("enum images contain constants"),
    };
    let mut order: Vec<&str> = Vec::new();
    let mut last: HashMap<&str, &perry_hir::EnumValue> = HashMap::new();
    for (key, value) in stores {
        if last.insert(key, value).is_none() {
            order.push(key);
        }
    }
    let image = Expr::New {
        byte_end: 0,
        class_name: class_name.to_string(),
        args: order.iter().map(|key| value_expr(last[key])).collect(),
        type_args: Vec::new(),
        byte_offset: 0,
        cap_args_appended: 0,
    };
    let mut descriptor = Descriptor::default();
    let shaped = !(ctx.program_has_worker
        && crate::target_layout::target_is_ilp32(ctx.target_triple))
        && descriptor.value(ctx, &image, 0).is_some();
    if !shaped {
        descriptor = Descriptor::default();
    }
    let mut replay = Descriptor::default();
    replay
        .u32(stores.len())
        .ok_or_else(|| anyhow::anyhow!("enum image: too many stores"))?;
    for (key, value) in stores {
        replay
            .u32(key.len())
            .ok_or_else(|| anyhow::anyhow!("enum image: key too long"))?;
        replay.bytes.extend_from_slice(key.as_bytes());
        replay
            .value(ctx, &value_expr(value), 0)
            .ok_or_else(|| anyhow::anyhow!("enum image: value"))?;
    }
    let index = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let function: String = ctx
        .func
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let name = format!("perry_enumimg_{function}_{index}");
    fn rodata(ctx: &mut FnCtx<'_>, global: String, bytes: &[u8]) -> String {
        let mut text = String::with_capacity(bytes.len() * 3);
        for byte in bytes {
            use std::fmt::Write;
            write!(&mut text, "\\{byte:02X}").unwrap();
        }
        ctx.typed_parse_rodata.push(format!(
            "@{global} = private unnamed_addr constant [{} x i8] c\"{text}\"",
            bytes.len()
        ));
        format!("@{global}")
    }
    let replay_ref = rodata(ctx, format!("{name}_stores"), &replay.bytes);
    let replay_len = replay.bytes.len().to_string();
    let (bytes_ref, len, shapes_ref, count) = if shaped {
        let bytes_ref = rodata(ctx, name.clone(), &descriptor.bytes);
        let shapes_ref = if ctx.program_has_worker {
            frame_shapes(ctx, &descriptor)
        } else {
            let records: Vec<String> = descriptor
                .shapes
                .iter()
                .map(ShapeRecord::constant)
                .collect();
            ctx.typed_parse_rodata.push(format!(
                "@{name}_shapes = private constant [{} x {SHAPE_TYPE}] [{}]",
                records.len(),
                records.join(", "),
            ));
            format!("@{name}_shapes")
        };
        (
            bytes_ref,
            descriptor.bytes.len().to_string(),
            shapes_ref,
            descriptor.shapes.len().to_string(),
        )
    } else {
        (
            "null".to_string(),
            "0".to_string(),
            "null".to_string(),
            "0".to_string(),
        )
    };
    Ok(ctx.block().call(
        DOUBLE,
        "js_enum_from_literal_descriptor",
        &[
            (PTR, bytes_ref.as_str()),
            (I32, len.as_str()),
            (PTR, shapes_ref.as_str()),
            (I32, count.as_str()),
            (PTR, replay_ref.as_str()),
            (I32, replay_len.as_str()),
            (I32, if strict { "1" } else { "0" }),
        ],
    ))
}

/// The shape table of a program that starts workers. Each agent then owns its
/// class keys and ShapeId slots (thread-local globals), whose addresses are no
/// link-time constant: a rodata table naming them is rejected by `ld -r`
/// (#10399: "TLS definition ... mismatches non-TLS reference"). The table is
/// built in the frame instead, as words at the record's LP64 field offsets
/// (`runtime::array::literal_descriptor::LiteralShape`), its two slot fields
/// holding this agent's addresses.
fn frame_shapes(ctx: &mut FnCtx<'_>, descriptor: &Descriptor) -> String {
    const RECORD_WORDS: usize = 8;
    let table = ctx
        .func
        .alloca_entry_array(I64, RECORD_WORDS * descriptor.shapes.len());
    for (i, record) in descriptor.shapes.iter().enumerate() {
        let keys = ctx.func.entry_tls_address(&record.keys);
        let shape_id = format!("@{}", record.shape_id);
        let base = 8 * RECORD_WORDS * i;
        let fields: [(LlvmType, usize, String); 10] = [
            (I32, 0, record.class_id.to_string()),
            (I32, 4, record.field_count.to_string()),
            (PTR, 8, keys),
            (PTR, 16, shape_id),
            (PTR, 24, record.raw_mask.clone()),
            (I32, 32, record.raw_mask_len.to_string()),
            (PTR, 40, record.pointer_mask.clone()),
            (I32, 48, record.pointer_mask_len.to_string()),
            (I32, 52, record.width.to_string()),
            (I64, 56, record.rep.to_string()),
        ];
        let blk = ctx.block();
        for (ty, offset, value) in &fields {
            let at = blk.gep(
                crate::types::I8,
                &table,
                &[(I64, &(base + offset).to_string())],
            );
            blk.store(ty, value, &at);
        }
    }
    table
}

#[cfg(test)]
pub(crate) mod tests;

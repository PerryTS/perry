//! Constant record trees retain their ordinary HIR shapes, but construction is
//! data-driven. A descriptor references the same rooted keys and typed ShapeId
//! globals as `new`; it never changes the representation used by property reads.

use std::collections::HashMap;

use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I32, PTR};

const MIN_NODES: usize = 256;
const MAX_DEPTH: usize = 128;
// Must match runtime::array::literal_descriptor::LiteralShape (repr(C)).
const SHAPE_TYPE: &str = "{ i32, i32, ptr, ptr, ptr, i32, ptr, i32 }";

#[derive(Default)]
struct Descriptor {
    bytes: Vec<u8>,
    shapes: Vec<String>,
    shape_indices: HashMap<String, u32>,
    nodes: usize,
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
        let index = u32::try_from(self.shapes.len()).ok()?;
        self.shapes.push(format!(
            "{SHAPE_TYPE} {{ i32 {class_id}, i32 {argc}, ptr @{keys}, ptr @{shape_id}, \
             ptr {raw_mask}, i32 {}, ptr {pointer_mask}, i32 {} }}",
            layout.raw_f64_mask_words.len(),
            layout.pointer_mask_words.len(),
        ));
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
            Expr::Array(elements) => {
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
            // Calls, spreads, computed properties, captures and user classes
            // all keep ordinary evaluation; no partially serialized expression
            // is ever emitted or evaluated.
            _ => return None,
        }
        Some(())
    }
}

pub(super) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Option<String> {
    if !matches!(expr, Expr::Array(_) | Expr::New { .. }) {
        return None;
    }
    // #10399: the `@<name>_shapes` table emitted below is a link-time
    // `constant` whose entries hold the ADDRESS of each class's
    // `perry_class_keys_*` / `perry_class_shape_id_*` globals. When the
    // program constructs a Worker those globals are thread-local so that each
    // thread builds its own module state — and the address of a thread-local
    // is not a link-time constant. LLVM emits the reference anyway and `ld -r`
    // rejects the object:
    //
    //   ld: perry_class_keys_<mod>____AnonShape_<hash>: TLS definition in
    //       unit2.o section .tbss mismatches non-TLS reference in unit7.o
    //
    // which is how three of prettier's plugins stopped linking. Fall back to
    // ordinary evaluation for worker-bearing programs; every other program
    // keeps this fast path untouched.
    if crate::codegen::program_has_worker() {
        return None;
    }
    let mut descriptor = Descriptor::default();
    descriptor.value(ctx, expr, 0)?;
    if descriptor.nodes < MIN_NODES || descriptor.shapes.is_empty() {
        return None;
    }
    let len = u32::try_from(descriptor.bytes.len()).ok()?;
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
    for byte in descriptor.bytes {
        use std::fmt::Write;
        write!(&mut bytes, "\\{byte:02X}").unwrap();
    }
    ctx.typed_parse_rodata.push(format!(
        "@{name} = private unnamed_addr constant [{len} x i8] c\"{bytes}\"",
    ));
    let count = descriptor.shapes.len();
    ctx.typed_parse_rodata.push(format!(
        "@{name}_shapes = private constant [{count} x {SHAPE_TYPE}] [{}]",
        descriptor.shapes.join(", "),
    ));
    Some(ctx.block().call(
        DOUBLE,
        "js_value_from_literal_descriptor",
        &[
            (PTR, &format!("@{name}")),
            (I32, &len.to_string()),
            (PTR, &format!("@{name}_shapes")),
            (I32, &count.to_string()),
        ],
    ))
}

#[cfg(test)]
mod tests;

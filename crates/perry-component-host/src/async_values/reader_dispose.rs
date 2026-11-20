//! Consume discarded canonical payloads using their original resource types.
//! Storage cleanup is separate: transferred handles are destroyed first, then
//! the existing allocation visitor releases the canonical copies.
use super::reader_types::Types;
use anyhow::{bail, ensure, Result};
use perry_wit::{
    canonical::{Layout, LayoutKind},
    Type, World,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Disposal {
    pub intrinsics: String,
    pub imports: String,
    pub exports: String,
    pub functions: String,
    seen: BTreeMap<u32, bool>,
}
impl Disposal {
    fn drop_handle(&mut self, id: u32, intrinsic: String) -> String {
        self.intrinsics += &format!("(core func $release{id} (canon {intrinsic}))\n");
        self.imports +=
            &format!("(import \"host\" \"release-{id}\" (func $release{id} (param i32)))\n");
        self.exports += &format!("(export \"release-{id}\" (func $release{id}))\n");
        format!("(call $release{id} (i32.load (local.get $p)))")
    }
    pub fn visit(
        &mut self,
        world: &World,
        ty: &Type,
        layout: &Layout,
        types: &mut Types<'_>,
        depth: usize,
    ) -> Result<bool> {
        ensure!(depth < 128, "async disposal type nesting exceeds 128");
        if let Type::Ref(key) = ty {
            return self.visit(
                world,
                &world
                    .types
                    .get(key)
                    .ok_or_else(|| anyhow::anyhow!("unknown disposal type"))?
                    .ty,
                layout,
                types,
                depth + 1,
            );
        }
        let id = layout.node_id();
        if let Some(needed) = self.seen.get(&id) {
            return Ok(*needed);
        }
        let mut body = String::new();
        match (ty, &layout.kind) {
            (Type::Own(key), _) => {
                body = self.drop_handle(id, format!("resource.drop {}", types.resource(key, 0)?));
            }
            (Type::Future(_), _) | (Type::Stream(_), _) => {
                let kind = if matches!(ty, Type::Future(_)) {
                    "future"
                } else {
                    "stream"
                };
                body =
                    self.drop_handle(id, format!("{kind}.drop-readable {}", types.value(ty, 0)?));
            }
            (Type::List(ty), LayoutKind::List { element }) => {
                if self.visit(world, ty, element, types, depth + 1)? {
                    body = format!(
                        r#"
                  (local.set $n (i32.load offset=4 (local.get $p)))
                  (local.set $q (i32.load (local.get $p)))
                  (block $done (loop $each (br_if $done (i32.eqz (local.get $n)))
                    (call $discard{} (local.get $q))
                    (local.set $q (i32.add (local.get $q) (i32.const {})))
                    (local.set $n (i32.sub (local.get $n) (i32.const 1))) (br $each)))"#,
                        element.node_id(),
                        element.size
                    );
                }
            }
            (Type::Record(fields), LayoutKind::Fields(layouts)) => {
                for (field, (offset, layout)) in fields.iter().zip(layouts) {
                    if self.visit(world, &field.ty, layout, types, depth + 1)? {
                        body += &format!(
                            "(call $discard{} (i32.add (local.get $p) (i32.const {offset})))\n",
                            layout.node_id()
                        );
                    }
                }
            }
            (Type::Tuple(fields), LayoutKind::Fields(layouts)) => {
                for (field, (offset, layout)) in fields.iter().zip(layouts) {
                    if self.visit(world, field, layout, types, depth + 1)? {
                        body += &format!(
                            "(call $discard{} (i32.add (local.get $p) (i32.const {offset})))\n",
                            layout.node_id()
                        );
                    }
                }
            }
            (
                _,
                LayoutKind::Variant {
                    tag_bytes,
                    payload_offset,
                    cases,
                },
            ) => {
                let values: Vec<_> = match ty {
                    Type::Option(ty) => vec![None, Some(ty.as_ref())],
                    Type::Result { ok, err } => vec![ok.as_deref(), err.as_deref()],
                    Type::Variant(cases) => cases.iter().map(|c| c.ty.as_ref()).collect(),
                    _ => bail!("unexpected disposal variant"),
                };
                let load = match tag_bytes {
                    1 => "i32.load8_u",
                    2 => "i32.load16_u",
                    4 => "i32.load",
                    _ => bail!("unexpected disposal tag"),
                };
                for (tag, (ty, case)) in values.into_iter().zip(cases).enumerate() {
                    if let (Some(ty), Some(case)) = (ty, case) {
                        if self.visit(world, ty, case, types, depth + 1)? {
                            body += &format!("(if (i32.eq (local.get $n) (i32.const {tag})) (then (call $discard{} (i32.add (local.get $p) (i32.const {payload_offset})))))\n", case.node_id());
                        }
                    }
                }
                if !body.is_empty() {
                    body = format!("(local.set $n ({load} (local.get $p)))\n{body}");
                }
            }
            _ => {}
        }
        self.functions +=
            &format!("(func $discard{id} (param $p i32) (local $n i32) (local $q i32) {body})\n");
        let needed = !body.is_empty();
        self.seen.insert(id, needed);
        Ok(needed)
    }
}

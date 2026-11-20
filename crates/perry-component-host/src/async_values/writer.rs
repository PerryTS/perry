//! Canonical factory/write functions share the reader's memory and nominal types.
use super::reader_types::Types;
use anyhow::{Context, Result};
use perry_wit::{
    canonical::{layout, LayoutKind},
    Field, Type, World,
};

pub(super) struct Generated {
    pub intrinsics: String,
    pub imports: String,
    pub exports: String,
    pub functions: String,
    pub interface: String,
}
pub(super) fn generate(
    world: &World,
    payload: Option<&Type>,
    stream: bool,
    reader: &Type,
    types: &mut Types<'_>,
) -> Result<Generated> {
    let reader_type = types.value(reader, 0)?;
    let factory = types.value(&Type::Tuple(vec![reader.clone(), Type::U32]), 0)?;
    let list = Type::List(Box::new(payload.cloned().unwrap_or(Type::U8)));
    let list_type = types.value(&list, 0)?;
    // Keep enough padding to force indirect task.return with every payload.
    let mut fields = (0..18)
        .map(|i| Field {
            name: format!("pad-{i}"),
            ty: Type::U32,
        })
        .collect::<Vec<_>>();
    fields.extend([
        Field {
            name: "status".into(),
            ty: Type::U32,
        },
        Field {
            name: "count".into(),
            ty: Type::U32,
        },
        Field {
            name: "remaining".into(),
            ty: list,
        },
    ]);
    let packet = Type::Record(fields);
    let plan = layout(world, &packet)?;
    let LayoutKind::Fields(fields) = &plan.kind else {
        unreachable!()
    };
    let packet_type = types.value(&packet, 0)?;
    let kind = if stream { "stream" } else { "future" };
    let mut functions = include_str!("writer.wat").to_string();
    for (name, value) in [
        ("@write-status@", fields[18].0.to_string()),
        ("@write-count@", fields[19].0.to_string()),
        ("@remaining@", fields[20].0.to_string()),
        (
            "@remaining-length@",
            fields[20]
                .0
                .checked_add(4)
                .context("writer packet offset overflow")?
                .to_string(),
        ),
        ("@write-packet-size@", plan.size.to_string()),
    ] {
        functions = functions.replace(name, &value);
    }
    Ok(Generated {
        intrinsics: format!(
            r#"
          (core func $writer-new (canon {kind}.new {reader_type}))
          (core func $write (canon {kind}.write {reader_type} async (memory $memory)))
          (core func $cancel-write (canon {kind}.cancel-write {reader_type}))
          (core func $drop-write (canon {kind}.drop-writable {reader_type}))
          (core func $writer-return (canon task.return (result {packet_type}) (memory $memory)))
        "#
        ),
        imports: r#"
          (import "host" "writer-new" (func $writer-new (result i64)))
          (import "host" "write" (func $write (param @read-params@) (result i32)))
          (import "host" "cancel-write" (func $cancel-write (param i32) (result i32)))
          (import "host" "drop-write" (func $drop-write (param i32)))
          (import "host" "writer-return" (func $writer-return (param i32)))
        "#
        .into(),
        exports: r#"
          (export "writer-new" (func $writer-new)) (export "write" (func $write))
          (export "cancel-write" (func $cancel-write)) (export "drop-write" (func $drop-write))
          (export "writer-return" (func $writer-return))
        "#
        .into(),
        functions,
        interface: format!(
            r#"
          (func (export "writer-new") (result {factory})
            (canon lift (core func $code "writer-new") (memory $memory) (post-return (core func $heap "free"))))
          (func (export "writer-write") async (param "writer" u32) (param "values" {list_type}) (param "count" u32) (param "id" u32) (result {packet_type})
            (canon lift (core func $code "writer-write") async (memory $memory) (realloc (core func $heap "realloc")) (callback (core func $code "writer-callback"))))
          (func (export "writer-close") (param "writer" u32)
            (canon lift (core func $code "writer-close")))
        "#
        ),
    })
}

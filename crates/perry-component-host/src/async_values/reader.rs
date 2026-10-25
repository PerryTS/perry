//! Build the canonical read/disposal helper composed into production hosts.
//! The persistent native driver keeps early-return producer tasks alive.
use super::{
    reader_cleanup,
    reader_dispose::Disposal,
    reader_types::{quote, Resources, Types},
    HEAP,
};
use anyhow::{ensure, Context, Result};
use perry_wit::{
    canonical::{layout, LayoutKind},
    Field, Type, World,
};

pub(super) fn generate(
    world: &World,
    payload: Option<&Type>,
    stream: bool,
    resources: &Resources,
    control: &str,
) -> Result<String> {
    ensure!(
        !resources
            .values()
            .any(|(interface, name)| interface == control
                || (interface.is_empty() && name == control)),
        "private async control import collides with resource import"
    );
    let reader = if stream {
        Type::Stream(payload.cloned().map(Box::new))
    } else {
        Type::Future(payload.cloned().map(Box::new))
    };
    // A list lifts any payload without handwritten flat ABI joins. Padding
    // forces the canonical task.return result to use an indirect pointer.
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
            name: "values".into(),
            ty: Type::List(Box::new(payload.cloned().unwrap_or(Type::U8))),
        },
        Field {
            name: "reader".into(),
            ty: Type::Option(Box::new(reader.clone())),
        },
    ]);
    let packet = Type::Record(fields);
    let plan = layout(world, &packet)?;
    let LayoutKind::Fields(fields) = &plan.kind else {
        unreachable!()
    };
    let (status, count, values, continuation) =
        (fields[18].0, fields[19].0, fields[20].0, fields[21].0);
    let LayoutKind::Variant { payload_offset, .. } = &fields[21].1.kind else {
        unreachable!()
    };
    let reader_offset = continuation
        .checked_add(*payload_offset)
        .context("async reader offset overflow")?;
    let LayoutKind::List { element } = &fields[20].1.kind else {
        unreachable!()
    };
    let element_size = if payload.is_some() { element.size } else { 0 };
    let mut types = Types::new(world, resources);
    let reader_type = types.value(&reader, 0)?;
    let packet_type = types.value(&packet, 0)?;
    let list = Type::List(Box::new(payload.cloned().unwrap_or(Type::U8)));
    let list_type = types.value(&list, 0)?;
    let mut disposal = Disposal::default();
    disposal.visit(world, &list, &fields[20].1, &mut types, 0)?;
    let kind = if stream { "stream" } else { "future" };
    let mut text = include_str!("reader.component.wat").to_string();
    let replacements = [
        ("@imports@", types.imports()),
        ("@types@", types.text),
        ("@control@", quote(control)),
        ("@list-type@", list_type),
        ("@list-id@", fields[20].1.node_id().to_string()),
        ("@dispose-intrinsics@", disposal.intrinsics),
        ("@dispose-imports@", disposal.imports),
        ("@dispose-exports@", disposal.exports),
        ("@dispose-functions@", disposal.functions),
        ("@reader@", reader_type),
        ("@packet@", packet_type),
        ("@kind@", kind.into()),
        ("@heap@", HEAP.replacen("(module", "(core module $heap", 1)),
        (
            "@read-params@",
            if stream { "i32 i32 i32" } else { "i32 i32" }.into(),
        ),
        (
            "@read-capacity@",
            if stream {
                "(i32.load offset=28 (local.get $f))"
            } else {
                ""
            }
            .into(),
        ),
        ("@stream@", u32::from(stream).to_string()),
        ("@has-payload@", u32::from(payload.is_some()).to_string()),
        ("@io-event@", if stream { "2" } else { "4" }.into()),
        ("@element-size@", element_size.to_string()),
        ("@packet-size@", plan.size.to_string()),
        ("@status@", status.to_string()),
        ("@count@", count.to_string()),
        ("@values@", values.to_string()),
        ("@values-length@", (values + 4).to_string()),
        ("@continuation@", continuation.to_string()),
        ("@reader-offset@", reader_offset.to_string()),
        ("@cleanup@", reader_cleanup::visitors(&plan)?),
        ("@cleanup-id@", plan.node_id().to_string()),
    ];
    for (key, value) in replacements {
        text = text.replace(key, &value);
    }
    ensure!(!text.contains('@'), "unexpanded async helper template");
    Ok(text)
}

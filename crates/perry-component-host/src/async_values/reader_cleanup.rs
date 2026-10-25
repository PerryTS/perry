//! Visitors reclaim canonical allocations after dynamic task.return lifts them.
//! Resource/future/stream handles have transferred to the embedding by then.
use anyhow::{bail, Result};
use perry_wit::canonical::{Layout, LayoutKind};
use std::collections::BTreeSet;

pub(super) fn visitors(root: &Layout) -> Result<String> {
    fn emit(layout: &Layout, seen: &mut BTreeSet<u32>, output: &mut String) -> Result<()> {
        if !seen.insert(layout.node_id()) {
            return Ok(());
        }
        let mut body = String::new();
        match &layout.kind {
            LayoutKind::Scalar => {}
            LayoutKind::String => body += "(call $free (i32.load (local.get $p)))",
            LayoutKind::List { element } => {
                emit(element, seen, output)?;
                body += &format!(
                    r#"
                  (local.set $n (i32.load offset=4 (local.get $p)))
                  (local.set $q (i32.load (local.get $p)))
                  (block $done (loop $each
                    (br_if $done (i32.eqz (local.get $n)))
                    (call $clean{} (local.get $q))
                    (local.set $q (i32.add (local.get $q) (i32.const {})))
                    (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                    (br $each)))
                  (call $free (i32.load (local.get $p)))"#,
                    element.node_id(),
                    element.size
                );
            }
            LayoutKind::Fields(fields) => {
                for (offset, field) in fields {
                    emit(field, seen, output)?;
                    body += &format!(
                        "(call $clean{} (i32.add (local.get $p) (i32.const {offset})))\n",
                        field.node_id()
                    );
                }
            }
            LayoutKind::Variant {
                tag_bytes,
                payload_offset,
                cases,
            } => {
                let load = match tag_bytes {
                    1 => "i32.load8_u",
                    2 => "i32.load16_u",
                    4 => "i32.load",
                    _ => bail!("unsupported canonical discriminant"),
                };
                body += &format!("(local.set $n ({load} (local.get $p)))\n");
                for (tag, case) in cases.iter().enumerate() {
                    if let Some(case) = case {
                        emit(case, seen, output)?;
                        body += &format!("(if (i32.eq (local.get $n) (i32.const {tag})) (then (call $clean{} (i32.add (local.get $p) (i32.const {payload_offset})))))\n", case.node_id());
                    }
                }
            }
        }
        *output += &format!(
            "(func $clean{} (param $p i32) (local $n i32) (local $q i32) {body})\n",
            layout.node_id()
        );
        Ok(())
    }
    let mut output = String::new();
    emit(root, &mut BTreeSet::new(), &mut output)?;
    Ok(output)
}

//! Visitors reclaim canonical allocations after dynamic task.return lifts them.
//! Resource/future/stream handles have transferred to the embedding by then.
use anyhow::{bail, Result};
use perry_wit::canonical::{Layout, LayoutKind};
use std::collections::BTreeMap;

pub(super) fn visitors(root: &Layout) -> Result<String> {
    fn emit(layout: &Layout, seen: &mut BTreeMap<u32, bool>, output: &mut String) -> Result<bool> {
        if let Some(needed) = seen.get(&layout.node_id()) {
            return Ok(*needed);
        }
        let mut body = String::new();
        match &layout.kind {
            LayoutKind::Scalar => {}
            LayoutKind::String => body += "(call $free (i32.load (local.get $p)))",
            LayoutKind::List { element } => {
                if emit(element, seen, output)? {
                    body += &format!(
                        r#"
                  (local.set $n (i32.load offset=4 (local.get $p)))
                  (local.set $q (i32.load (local.get $p)))
                  (block $done (loop $each
                    (br_if $done (i32.eqz (local.get $n)))
                    (call $clean{} (local.get $q))
                    (local.set $q (i32.add (local.get $q) (i32.const {})))
                    (local.set $n (i32.sub (local.get $n) (i32.const 1)))
                    (br $each)))"#,
                        element.node_id(),
                        element.size
                    );
                }
                body += "(call $free (i32.load (local.get $p)))";
            }
            LayoutKind::Fields(fields) => {
                for (offset, field) in fields {
                    if emit(field, seen, output)? {
                        body += &format!(
                            "(call $clean{} (i32.add (local.get $p) (i32.const {offset})))\n",
                            field.node_id()
                        );
                    }
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
                for (tag, case) in cases.iter().enumerate() {
                    if let Some(case) = case {
                        if emit(case, seen, output)? {
                            body += &format!("(if (i32.eq (local.get $n) (i32.const {tag})) (then (call $clean{} (i32.add (local.get $p) (i32.const {payload_offset})))))\n", case.node_id());
                        }
                    }
                }
                if !body.is_empty() {
                    body = format!("(local.set $n ({load} (local.get $p)))\n{body}");
                }
            }
        }
        *output += &format!(
            "(func $clean{} (param $p i32) (local $n i32) (local $q i32) {body})\n",
            layout.node_id()
        );
        let needed = !body.is_empty();
        seen.insert(layout.node_id(), needed);
        Ok(needed)
    }
    let mut output = String::new();
    emit(root, &mut BTreeMap::new(), &mut output)?;
    Ok(output)
}

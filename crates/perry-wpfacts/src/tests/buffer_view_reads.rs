//! A Buffer's `subarray` / `slice` is a Buffer over the receiver's bytes,
//! `fill` / `swap16` return the receiver: a read through the result is a
//! Buffer read (once a container of primitives whose `readInt32BE` held
//! nothing, and `sum += v.readInt32BE(..)` stayed `[0,0]`). A spread call
//! (`subarray(...args)`) makes the same view.

use super::*;
use crate::FactsConfig;

fn rep_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rep "))
        .cloned()
        .collect()
}

fn program(view: &str) -> String {
    format!(
        r#"
function scan(v: Buffer): number {{
  let sum = 0;
  for (let i = 0; i < 4; i++) sum += v.readInt32BE(i * 4);
  return sum;
}}
const args: [number, number] = [4, 20];
const b = Buffer.alloc(32){view};
b.writeInt32BE(5, 0);
console.log(scan(b));
"#
    )
}

fn zero(ls: &[String]) -> bool {
    ls.iter().any(|l| l.contains("range=[0,0]"))
}

#[test]
fn a_read_through_a_buffer_view_is_no_constant() {
    for view in [
        ".subarray(16)",
        ".slice(16)",
        ".subarray(...args)",
        ".slice(...args)",
        ".fill(1)",
        ".swap16()",
        "",
    ] {
        let ls = rep_lines(&program(view));
        assert!(!zero(&ls), "{view}: {ls:#?}");
    }
}

/// The probe prints rep lines for this program, so the test above does not
/// pass on an empty probe.
#[test]
fn the_probe_has_rep_lines() {
    assert!(!rep_lines(&program(".subarray(16)")).is_empty());
}

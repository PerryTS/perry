//! A Buffer's `subarray` / `slice` is a Buffer over the receiver's bytes,
//! `fill` / `swap16` return the receiver: a read through the result is a
//! Buffer read (once a container of primitives whose `readInt32BE` held
//! nothing, and `sum += v.readInt32BE(..)` stayed `[0,0]`). A numeric rule
//! one of whose operands holds nothing gives its result no interval
//! (`solve/empty_operands.rs`): the spread call still takes the undescribed
//! path, and the fallback covers it.

use super::*;
use crate::FactsConfig;

fn rep_lines(src: &str, fallback: bool) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            no_empty_operands: !fallback,
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
    for view in [".subarray(16)", ".slice(16)", ".fill(1)", ".swap16()", ""] {
        let ls = rep_lines(&program(view), false);
        assert!(!zero(&ls), "{view}: {ls:#?}");
    }
}

/// The spread call is undescribed: without the fallback its read holds
/// nothing and the sum is `[0,0]` (the negative control); with it, no
/// interval.
#[test]
fn an_operand_that_holds_nothing_gives_no_interval() {
    let src = program(".subarray(...args)");
    assert!(zero(&rep_lines(&src, false)), "control");
    let ls = rep_lines(&src, true);
    assert!(!zero(&ls), "{ls:#?}");
}

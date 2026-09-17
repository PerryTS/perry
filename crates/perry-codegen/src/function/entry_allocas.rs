//! #10463: every `alloca` a function emits lives in its LLVM entry block.
//!
//! LLVM lowers an `alloca` outside the entry block as a runtime stack-pointer
//! bump that is not undone until the function returns. Inside a loop every
//! iteration therefore consumes stack for good: `d.setTime(i)` took 16 B per
//! iteration and a two-million-iteration loop died with SIGSEGV at a point
//! that moved with `ulimit -s`. #167 added
//! [`LlFunction::alloca_entry_array`](super::LlFunction::alloca_entry_array)
//! for one family of call sites; eight sibling lowerings kept emitting
//! `alloca [N x double]` into whatever block was current, and the HIR-level
//! cross-module inliner copied them into callers' loops (date-fns
//! `addMinutes`).
//!
//! Fixing call sites one at a time is how the class survived #167, so the
//! invariant is enforced where every function body is finalized:
//! [`LlFunction::for_each_final_item`](super::LlFunction::for_each_final_item),
//! the single funnel both the textual and the native backends consume. An
//! entry-block slot comes from `LlFunction::alloca_entry*`, which splices it
//! ahead of block 0's instructions. The only other legal spelling is
//! `LlBlock::alloca` while block 0 is current (the parameter prologues). Any
//! `alloca` instruction the stream places after that point is refused.

use crate::inst::LlInst;

/// Refuse `inst` if it is an `alloca` outside the entry block.
///
/// `in_entry_block` is true while the stream is still inside the LLVM entry
/// block: the caller starts it `true` for block 0 and `false` for every other
/// block, and a label inside an instruction stream (the invoke-EH
/// continuation `emit_inline_label` writes) ends the entry block part-way
/// through block 0, so it is cleared here.
pub(super) fn refuse_alloca_outside_entry_block(
    function: &str,
    block: &str,
    inst: &LlInst,
    in_entry_block: &mut bool,
) {
    match inst {
        LlInst::Alloca { .. } if !*in_entry_block => {
            let mut line = String::new();
            inst.render_into(&mut line);
            refuse(function, block, &line);
        }
        LlInst::Raw(text) => {
            for line in text.split('\n') {
                if is_label_line(line) {
                    *in_entry_block = false;
                } else if !*in_entry_block && is_alloca_line(line) {
                    refuse(function, block, line);
                }
            }
        }
        _ => {}
    }
}

/// A flush-left `name:` line — the same column-0 rule the IR-reading scripts
/// anchor labels on (see `LlBlock::emit_inline_label`). Instructions carry a
/// two-space indent and never end in `:`.
fn is_label_line(line: &str) -> bool {
    line.ends_with(':')
        && line
            .as_bytes()
            .first()
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b';')
}

/// `%reg = alloca …`, whatever the allocated type.
fn is_alloca_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with('%')
        && line
            .split_once(" = ")
            .is_some_and(|(_, rhs)| rhs.starts_with("alloca "))
}

#[cold]
#[inline(never)]
fn refuse(function: &str, block: &str, line: &str) -> ! {
    panic!(
        "perry-codegen: `{}` is emitted in block `{block}` of @{function}, outside the \
         function's entry block. A non-entry alloca bumps the stack pointer at run time and \
         is not released until the function returns, so every loop iteration through it \
         consumes stack until the process dies with SIGSEGV (#167, #10463). Allocate the \
         slot with `LlFunction::alloca_entry` / `alloca_entry_array` instead.",
        line.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::super::LlFunction;
    use crate::types::{DOUBLE, I64, PTR};

    fn probe() -> LlFunction {
        let mut f = LlFunction::new("perry_fn_alloca_probe", DOUBLE, Vec::new());
        let _ = f.create_block("entry");
        f
    }

    fn entry_prologue_ends_at(ir: &str, first_non_entry_label: &str) -> usize {
        ir.lines()
            .position(|line| line == format!("{first_non_entry_label}:"))
            .unwrap_or_else(|| panic!("no `{first_non_entry_label}:` label in:\n{ir}"))
    }

    /// The control: the helpers put the slot in block 0 even when the block
    /// being lowered is a loop body, and the body keeps only the uses.
    #[test]
    fn entry_helpers_hoist_the_slot_out_of_the_current_block() {
        let mut f = probe();
        let body_label = f.create_block("for.body").label.clone();
        let buf = f.alloca_entry_array(DOUBLE, 2);
        let out = f.alloca_entry(I64);
        {
            let blk = f.block_mut(1).unwrap();
            let slot = blk.gep(DOUBLE, &buf, &[(I64, "1")]);
            blk.store(DOUBLE, "0.0", &slot);
            blk.call(DOUBLE, "js_consume", &[(PTR, &buf), (PTR, &out)]);
            blk.ret(DOUBLE, "0.0");
        }
        f.block_mut(0).unwrap().br(&body_label);
        let ir = f.to_ir();
        let body_starts = entry_prologue_ends_at(&ir, &body_label);
        let alloca_lines: Vec<usize> = ir
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(" = alloca "))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(alloca_lines.len(), 2, "both slots rendered:\n{ir}");
        assert!(
            alloca_lines.iter().all(|&i| i < body_starts),
            "every alloca must precede the loop body's label:\n{ir}"
        );
    }

    /// The bug shape the eight #10463 lowerings emitted: a raw `alloca` text
    /// line in a loop body.
    #[test]
    #[should_panic(expected = "outside the function's entry block")]
    fn a_raw_alloca_in_a_loop_body_is_refused() {
        let mut f = probe();
        let _ = f.create_block("for.body");
        let blk = f.block_mut(1).unwrap();
        let buf = blk.next_reg();
        blk.emit_raw(format!("{buf} = alloca [1 x double]"));
        blk.call(DOUBLE, "js_date_apply_setter", &[(PTR, &buf)]);
        let _ = f.to_ir();
    }

    /// The typed spelling of the same mistake (`blk.alloca` in a non-entry
    /// block — what `Expr::ArraySplice`'s out-parameter used).
    #[test]
    #[should_panic(expected = "outside the function's entry block")]
    fn a_typed_alloca_in_a_non_entry_block_is_refused() {
        let mut f = probe();
        let _ = f.create_block("splice");
        let _ = f.block_mut(1).unwrap().alloca(I64);
        let _ = f.to_ir();
    }

    /// Multi-line raw payloads are split and checked line by line.
    #[test]
    #[should_panic(expected = "outside the function's entry block")]
    fn an_alloca_inside_a_multi_line_raw_payload_is_refused() {
        let mut f = probe();
        let _ = f.create_block("body");
        f.block_mut(1)
            .unwrap()
            .emit_raw("%a = add i64 1, 2\n  %b = alloca double, align 8");
        let _ = f.to_ir();
    }

    /// An inline label ends the entry block part-way through block 0: an
    /// `alloca` after it is in a different LLVM basic block.
    #[test]
    #[should_panic(expected = "outside the function's entry block")]
    fn an_alloca_after_an_inline_label_in_block_zero_is_refused() {
        let mut f = probe();
        let blk = f.block_mut(0).unwrap();
        blk.insts_mut()
            .push(crate::inst::LlInst::Raw("eh.cont0:".to_string()));
        let _ = blk.alloca(DOUBLE);
        let _ = f.to_ir();
    }

    /// The native backend consumes the item stream, not the text: it is
    /// refused there too.
    #[test]
    #[should_panic(expected = "outside the function's entry block")]
    fn the_native_item_stream_refuses_it_too() {
        let mut f = probe();
        let _ = f.create_block("body");
        let _ = f.block_mut(1).unwrap().alloca(DOUBLE);
        let _ = f.for_each_final_item::<()>(&mut |_| Ok(()));
    }

    /// The prologue spelling stays legal: `LlBlock::alloca` while block 0 is
    /// current, before any inline label.
    #[test]
    fn a_typed_alloca_in_the_entry_prologue_is_accepted() {
        let mut f = probe();
        let blk = f.block_mut(0).unwrap();
        let slot = blk.alloca(DOUBLE);
        blk.store(DOUBLE, "0.0", &slot);
        blk.ret(DOUBLE, "0.0");
        assert!(f.to_ir().contains(" = alloca double"));
    }
}

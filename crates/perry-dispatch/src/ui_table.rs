//! `PERRY_UI_TABLE` — receiver-less perry/ui calls (constructors + setters).
//!
//! The row data is large enough that the single literal crossed the 2000-line
//! file-size gate, so it is split across `ui_table/part_a.rs` and
//! `ui_table/part_b.rs`. The two halves are concatenated at compile time below
//! so `PERRY_UI_TABLE` stays a `&'static [MethodRow]` — every existing consumer
//! (LLVM/JS/WASM emit, `perry-runtime/build.rs`, the dispatch-drift test) keeps
//! treating it as a flat static slice.

use super::*;

mod part_a;
mod part_b;

use part_a::PERRY_UI_TABLE_PART_A;
use part_b::PERRY_UI_TABLE_PART_B;

const PERRY_UI_TABLE_LEN: usize = PERRY_UI_TABLE_PART_A.len() + PERRY_UI_TABLE_PART_B.len();

const fn build_perry_ui_table() -> [MethodRow; PERRY_UI_TABLE_LEN] {
    // MethodRow is Copy; seed with the first row then overwrite every slot.
    let mut out = [PERRY_UI_TABLE_PART_A[0]; PERRY_UI_TABLE_LEN];
    let mut i = 0;
    let mut j = 0;
    while j < PERRY_UI_TABLE_PART_A.len() {
        out[i] = PERRY_UI_TABLE_PART_A[j];
        i += 1;
        j += 1;
    }
    j = 0;
    while j < PERRY_UI_TABLE_PART_B.len() {
        out[i] = PERRY_UI_TABLE_PART_B[j];
        i += 1;
        j += 1;
    }
    out
}

const PERRY_UI_TABLE_ARR: [MethodRow; PERRY_UI_TABLE_LEN] = build_perry_ui_table();

pub const PERRY_UI_TABLE: &[MethodRow] = &PERRY_UI_TABLE_ARR;

//! Entry-block cache of the cells a closure body reads through its captures.
//!
//! A captured box is one capture slot per binding; a captured scope object is
//! ONE capture slot shared by every binding of its group. The cache roots one
//! shadow slot per capture slot (not per binding) and derives each binding's
//! cell address from the reloaded base, so the reload pass re-derives it below
//! every collection point (`add`/`inttoptr` are transparent derivations).

use std::collections::{BTreeMap, HashMap};

use crate::expr::TrustedBoxCapturePtr;
use crate::function::LlFunction;
use crate::scope_env::ScopeMap;
use crate::types::{I32, I64, I8, PTR};

/// Cache `captures` (binding id, capture index) at entry. With `validate`, each
/// capture word is resolved through the runtime validator first (the public
/// body); without it the dispatcher already validated the layout (the private
/// exact-arrow clone).
pub(crate) fn cache_capture_cells(
    lf: &mut LlFunction,
    captures: &[(u32, u32)],
    scope_map: &ScopeMap,
    target_triple: &str,
    validate: bool,
) -> HashMap<u32, TrustedBoxCapturePtr> {
    let mut out = HashMap::new();
    let mut by_index: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (id, index) in captures {
        by_index.entry(*index).or_default().push(*id);
    }
    if by_index.is_empty() {
        return out;
    }
    let header_size = crate::target_layout::closure_header_size_bytes(target_triple).to_string();
    for (index, mut ids) in by_index {
        ids.sort_unstable();
        let scoped = ids.iter().any(|id| scope_map.slot(*id).is_some());
        let mut bits = {
            let blk = lf.block_mut(0).expect("closure body has an entry block");
            let closure_ptr = blk.inttoptr(I64, "%this_closure");
            let captures_base = blk.gep(I8, &closure_ptr, &[(I64, &header_size)]);
            let capture_slot = blk.gep(I64, &captures_base, &[(I64, &index.to_string())]);
            let raw = blk.load(I64, &capture_slot);
            if !validate {
                raw
            } else if scoped {
                blk.call(I64, "js_scope_capture_base", &[(I64, &raw)])
            } else {
                blk.call(I64, "js_box_capture_cell_ptr", &[(I64, &raw)])
            }
        };
        // The cell (or scope object) is movable: root it once for the whole
        // invocation and derive every cached SSA value from a root load.
        if let Some(root_index) = lf.reserve_shadow_slot() {
            let slot = lf.alloca_entry(I64);
            let blk = lf.block_mut(0).expect("closure entry");
            blk.store(I64, &bits, &slot);
            blk.call_void(
                "js_shadow_slot_bind",
                &[(I32, &root_index.to_string()), (PTR, &slot)],
            );
            bits = blk.load(I64, &slot);
        }
        let blk = lf.block_mut(0).expect("closure entry");
        for id in ids {
            let cell_bits = match scope_map.slot(id) {
                Some(slot) if slot.index > 0 => blk.add(
                    I64,
                    &bits,
                    &crate::scope_env::access::slot_offset(slot.index).to_string(),
                ),
                _ => bits.clone(),
            };
            let ptr = blk.inttoptr(I64, &cell_bits);
            out.insert(
                id,
                TrustedBoxCapturePtr {
                    bits: bits.clone(),
                    ptr,
                    cell_bits,
                },
            );
        }
    }
    out
}

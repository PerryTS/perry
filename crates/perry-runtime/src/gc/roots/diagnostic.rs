//! Diagnostic arm of the registered-root visitor. Ordinary collector modes
//! retain their direct calls; only this arm invokes a callback.
use super::*;

pub(in crate::gc) enum DiagnosticRootSlot {
    Nanbox(u64),
    Tagged(usize, u64),
    Metadata(usize),
}

pub(in crate::gc) enum DiagnosticRootContext<'a> {
    Verify {
        verifier: EvacuationVerifier<'a>,
        surface: &'static str,
    },
    CopyingMark(&'a mut CopyingNurseryCollector),
}

pub(in crate::gc) type DiagnosticRootCallback =
    fn(&mut DiagnosticRootContext<'_>, DiagnosticRootSlot) -> Option<u64>;

pub(super) fn verify_root(
    context: &mut DiagnosticRootContext<'_>,
    slot: DiagnosticRootSlot,
) -> Option<u64> {
    let DiagnosticRootContext::Verify { verifier, surface } = context else {
        unreachable!()
    };
    match slot {
        DiagnosticRootSlot::Nanbox(bits) => {
            if let Some(new_bits) = verifier.stale_nanboxed_value(bits) {
                panic_stale_forwarded_reference(*verifier, surface, 0, bits, new_bits);
            }
        }
        DiagnosticRootSlot::Tagged(addr, tag) => {
            if let Some(new_addr) = verifier.stale_raw_addr(addr) {
                panic_stale_forwarded_reference(
                    *verifier,
                    surface,
                    0,
                    tag | (addr as u64 & POINTER_MASK),
                    tag | (new_addr as u64 & POINTER_MASK),
                );
            }
        }
        DiagnosticRootSlot::Metadata(addr) => {
            if let Some(new_addr) = verifier.stale_raw_addr(addr) {
                panic_stale_forwarded_reference(
                    *verifier,
                    surface,
                    0,
                    addr as u64,
                    new_addr as u64,
                );
            }
        }
    }
    None
}

pub(super) fn copying_mark(
    context: &mut DiagnosticRootContext<'_>,
    slot: DiagnosticRootSlot,
) -> Option<u64> {
    let DiagnosticRootContext::CopyingMark(collector) = context else {
        unreachable!()
    };
    match slot {
        DiagnosticRootSlot::Nanbox(bits) => {
            decode_nanboxed_root_word(bits).and_then(|word| collector.visit_root_word::<true>(word))
        }
        DiagnosticRootSlot::Tagged(addr, _) => collector
            .visit_raw_addr::<true>(addr)
            .map(|addr| addr as u64),
        DiagnosticRootSlot::Metadata(_) => None,
    }
}

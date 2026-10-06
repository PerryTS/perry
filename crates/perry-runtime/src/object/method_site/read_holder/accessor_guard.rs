//! Runtime backend for the shared accessor guard program. Admission proves
//! worker exclusion and accessor kind for these exact words. No collecting
//! or reentrant edge occurs before the program completes.
use super::*;
use crate::codegen_abi::accessor_guards::{self, AccessorGuards};

struct Admitted<'a, const INLINE_ONLY: bool> {
    words: &'a HolderEntry,
    token: i64,
    kind: u64,
    getter: usize,
}

impl<const INLINE_ONLY: bool> AccessorGuards for Admitted<'_, INLINE_ONLY> {
    type Failure = ();

    #[inline(always)]
    fn receiver(&mut self) -> Result<(), ()> {
        if self.token as u32 == 0 || self.words[HOLDER_RECV] != self.token {
            Err(())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    fn kind(&mut self) -> Result<(), ()> {
        // The constructor's admission proof is about these same words.
        Ok(())
    }

    #[inline(always)]
    fn holder(&mut self) -> Result<(), ()> {
        if unsafe { shape_word(self.words[HOLDER_OBJ] as usize) } == self.words[HOLDER_SHAPE] as u32
        {
            Ok(())
        } else {
            Err(())
        }
    }

    #[inline(always)]
    fn callable(&mut self) -> Result<(), ()> {
        // Worker exclusion is also supplied by admission. Inline consumers
        // cannot derive a spill getter or call a setter-only pair.
        if INLINE_ONLY
            && (self.words[HOLDER_HOP_SHAPES] == 0 || self.kind as u32 & HOLDER_SLOT_SPILL != 0)
        {
            Err(())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    fn lane(&mut self) -> Result<(), ()> {
        let lane = crate::value::POINTER_TAG | self.words[HOLDER_HOPS] as u64;
        unsafe {
            if holder_slot_value(self.words[HOLDER_OBJ] as usize, self.kind as u32) != Some(lane) {
                return Err(());
            }
            self.getter = if !INLINE_ONLY
                && (self.kind & HOLDER_ACCESSOR_DEEP != 0
                    || self.kind as u32 & HOLDER_SLOT_SPILL != 0)
            {
                crate::object::accessor_pair::site_getter_word_of_value(lane).ok_or(())?
            } else {
                self.words[HOLDER_HOP_SHAPES] as usize
            };
        }
        Ok(())
    }
}

/// Run the shared program and consume its result in place. Keeping the
/// consumer here avoids a getter/pair tuple round trip on the primary hit.
///
/// # Safety
/// The caller has checked worker exclusion and accessor kind on `words`,
/// and `kind` is their current kind word. Nothing may mutate or collect
/// between that admission and this call. The receiver token is current.
#[inline(always)]
pub(super) unsafe fn with_answer<const INLINE_ONLY: bool, R>(
    words: &HolderEntry,
    token: i64,
    kind: u64,
    consume: impl FnOnce(usize, usize) -> R,
) -> Option<R> {
    let mut guard = Admitted::<INLINE_ONLY> {
        words,
        token,
        kind,
        getter: 0,
    };
    accessor_guards::validate(&mut guard).ok()?;
    Some(consume(guard.getter, words[HOLDER_HOPS] as usize))
}

// Saved entries share one validator body. The primary collecting hit uses
// `with_answer` directly so its guard/consumer sequence stays unchanged.
#[cold]
#[inline(never)]
pub(super) unsafe fn validated_accessor<const INLINE_ONLY: bool>(
    words: &HolderEntry,
    token: i64,
) -> Option<(usize, usize)> {
    let kind = words[HOLDER_KIND] as u64;
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0
        || kind & HOLDER_ACCESSOR == 0
        || kind & HOLDER_ACCESSOR_DEEP != 0
    {
        return None;
    }
    with_answer::<INLINE_ONLY, _>(words, token, kind, |getter, pair| (getter, pair))
}

#[cfg(test)]
mod tests;

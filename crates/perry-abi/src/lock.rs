//! DEC 250: the intrinsic prototype lock, as one decision.
//!
//! The runtime asks it before a mutation of a sealed intrinsic prototype
//! reaches the object (the key's class read from the live layout's image
//! prefix); the whole-program facts ask it whether a read on an intrinsic
//! chain is final, so that no write the program can make changes what it
//! finds. Both sides classify the key and name the holder; the decision of
//! what the lock refuses lives here only.

use crate::BuiltinProto;

/// A locked intrinsic prototype: a [`BuiltinProto`] family's prototype, one
/// of the built-in iterator prototypes or one of the two generator prototypes
/// the runtime seals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockHolder {
    Proto(BuiltinProto),
    /// `%IteratorPrototype%`.
    Iterator,
    /// `%ArrayIteratorPrototype%`.
    ArrayIterator,
    /// `%GeneratorPrototype%`.
    Generator,
    /// `%AsyncGeneratorPrototype%`.
    AsyncGenerator,
    /// `%MapIteratorPrototype%`.
    MapIterator,
    /// `%SetIteratorPrototype%`.
    SetIterator,
    /// `%StringIteratorPrototype%`.
    StringIterator,
    /// `%RegExpStringIteratorPrototype%`.
    RegExpStringIterator,
}

impl LockHolder {
    /// The prototypes on which the program adds no key (DEC 250 (i)):
    /// compiled code relies on keys being absent there (own-key misses, "a
    /// plain object is not iterable", `IteratorClose` finding no `return`).
    #[inline(always)]
    pub const fn add_locked(self) -> bool {
        matches!(
            self,
            LockHolder::Proto(BuiltinProto::Object | BuiltinProto::Function)
                | LockHolder::Iterator
                | LockHolder::ArrayIterator
        )
    }

    /// The prototypes that never gain an array index (DEC 250 (i)): the
    /// add-locked ones and `%Array.prototype%`, which has no element room
    /// (array hole and out-of-bounds reads rely on it).
    #[inline(always)]
    pub const fn index_locked(self) -> bool {
        self.add_locked() || matches!(self, LockHolder::Proto(BuiltinProto::Array))
    }
}

/// The key a mutation names, as the lock classifies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockKey {
    /// `constructor`: patchable on every built-in prototype (owner ruling
    /// 2026-11-03; Next.js redefines `Date.prototype.constructor`). The key
    /// is exempt, not one mutation of it: after its (admitted) delete,
    /// defining it again is an add of the same exempt key.
    Constructor,
    /// An own key the realm image gave the prototype.
    Image,
    /// An array index the image does not give the prototype.
    Index,
    /// Any other key (not own, or own because the program added it).
    Absent,
}

/// What a mutation does to the prototype it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockMutation {
    /// A value store into an existing own key.
    Replace,
    /// `delete`.
    Delete,
    /// `Object.defineProperty` of an own key (of a key that is not own: an
    /// [`LockMutation::Add`]).
    Redefine,
    /// A key-add, by a store or a definition.
    Add,
    /// `Object.setPrototypeOf` / `__proto__ =`.
    Relink,
}

/// Does the lock refuse `mutation` of `key` on `holder`?
#[inline(always)]
pub const fn refuses(holder: LockHolder, key: LockKey, mutation: LockMutation) -> bool {
    match (key, mutation) {
        (_, LockMutation::Relink) => true,
        (LockKey::Constructor, _) => false,
        (LockKey::Image, _) => true,
        // Not own: nothing to replace, delete or redefine.
        (LockKey::Index | LockKey::Absent, LockMutation::Replace)
        | (LockKey::Index | LockKey::Absent, LockMutation::Delete)
        | (LockKey::Index | LockKey::Absent, LockMutation::Redefine) => false,
        (LockKey::Index, LockMutation::Add) => holder.index_locked(),
        (LockKey::Absent, LockMutation::Add) => holder.add_locked(),
    }
}

/// Is what a lookup of `key` finds on `holder` final: does the lock refuse
/// every mutation that changes it (an own key's value, attributes and
/// presence; an absent key's absence)? What a final answer leaves to the
/// next prototype on the chain is for the caller to ask there.
#[inline(always)]
pub const fn final_on(holder: LockHolder, key: LockKey) -> bool {
    match key {
        LockKey::Constructor => false,
        LockKey::Image => {
            refuses(holder, key, LockMutation::Replace)
                && refuses(holder, key, LockMutation::Delete)
                && refuses(holder, key, LockMutation::Redefine)
        }
        LockKey::Index | LockKey::Absent => refuses(holder, key, LockMutation::Add),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_table() {
        let array = LockHolder::Proto(BuiltinProto::Array);
        let string = LockHolder::Proto(BuiltinProto::String);
        let object = LockHolder::Proto(BuiltinProto::Object);
        let function = LockHolder::Proto(BuiltinProto::Function);
        let all = [
            array,
            string,
            object,
            function,
            LockHolder::Iterator,
            LockHolder::ArrayIterator,
            LockHolder::Generator,
            LockHolder::AsyncGenerator,
            LockHolder::MapIterator,
            LockHolder::SetIterator,
            LockHolder::StringIterator,
            LockHolder::RegExpStringIterator,
        ];
        for h in all {
            for m in [
                LockMutation::Replace,
                LockMutation::Delete,
                LockMutation::Redefine,
            ] {
                assert!(refuses(h, LockKey::Image, m));
                assert!(!refuses(h, LockKey::Constructor, m));
                assert!(!refuses(h, LockKey::Absent, m));
            }
            assert!(refuses(h, LockKey::Absent, LockMutation::Relink));
            assert!(!refuses(h, LockKey::Constructor, LockMutation::Add));
            assert!(final_on(h, LockKey::Image));
            assert!(!final_on(h, LockKey::Constructor));
        }
        for h in [
            object,
            function,
            LockHolder::Iterator,
            LockHolder::ArrayIterator,
        ] {
            assert!(refuses(h, LockKey::Absent, LockMutation::Add));
            assert!(final_on(h, LockKey::Absent));
        }
        for h in [
            array,
            string,
            LockHolder::Generator,
            LockHolder::AsyncGenerator,
            LockHolder::MapIterator,
            LockHolder::SetIterator,
            LockHolder::StringIterator,
            LockHolder::RegExpStringIterator,
        ] {
            assert!(!refuses(h, LockKey::Absent, LockMutation::Add));
            assert!(!final_on(h, LockKey::Absent));
        }
        assert!(final_on(array, LockKey::Index));
        assert!(final_on(object, LockKey::Index));
        assert!(!final_on(string, LockKey::Index));
        assert!(!final_on(LockHolder::Generator, LockKey::Index));
        assert!(!final_on(LockHolder::MapIterator, LockKey::Index));
    }
}

//! A runtime site's memo for one fixed own property name: on receivers whose
//! header word is `(class_id | ShapeId << 32)` = `word`, the name is an own
//! plain DATA property at key position `slot` (an inline slot, or for a site
//! that reads through the shape's live bound, a spill position).
//!
//! This is the runtime-side form of the method-site memo's own entry
//! (`method_site`), for runtime code that reads and writes a property by a
//! name fixed in its source (an emitter's `_events`, `_eventsCount`): the
//! entry is facts of ONE receiver word, compared on every use, and nothing is
//! keyed on a class id, an address or the name alone.
//!
//! # Why an entry cannot go stale
//!
//! A ShapeId names one immutable key list, one descriptor state and one
//! `[[Prototype]]` (#11342), and ShapeIds are never reused. So "`name` is own
//! data at inline slot `s`" is a fact of the word: a shadowing accessor, a
//! descriptor, a delete or a re-layout moves the receiver to another ShapeId
//! and the word stops matching. The one in-place change a ShapeId admits is a
//! stable-tombstone receiver's (#9064) hole, so a hit whose slot holds
//! `TAG_HOLE` is no answer. A hit says nothing about writability beyond the
//! key's attributes (an identity fact); a writer also refuses a frozen
//! receiver and stores through the field-representation funnel.
//!
//! The memo holds two integers per way and never a pointer, so it is not a
//! GC root. It is per thread, and so per agent, like the ShapeIds it holds.

use crate::object::ObjectHeader;
use std::cell::Cell;

/// Ways per memo: a plain receiver and one other shape (an emitter and an
/// instance of a class extending it).
const WAYS: usize = 2;

/// No receiver word is all-ones.
const EMPTY: u64 = u64::MAX;

#[derive(Clone, Copy)]
struct Way {
    word: u64,
    slot: u32,
}

pub(crate) struct OwnSlotMemo {
    ways: [Cell<Way>; WAYS],
    next: Cell<u8>,
}

impl OwnSlotMemo {
    pub(crate) const fn new() -> Self {
        const W: Way = Way {
            word: EMPTY,
            slot: 0,
        };
        OwnSlotMemo {
            ways: [Cell::new(W), Cell::new(W)],
            next: Cell::new(0),
        }
    }

    /// The memo's inline slot for `obj`, when `obj`'s word was primed.
    ///
    /// # Safety
    /// `obj` is a live, unforwarded ordinary `ObjectHeader`.
    #[inline(always)]
    pub(crate) unsafe fn slot(&self, obj: *const ObjectHeader) -> Option<u32> {
        let word = std::ptr::read(obj as *const u64);
        self.ways
            .iter()
            .map(Cell::get)
            .find(|way| way.word == word)
            .map(|way| way.slot)
    }

    /// Record that the memo's name is an own plain data property at inline
    /// slot `slot` of `obj`'s shape. A receiver whose stamp is not an
    /// ordinary-band ShapeId (unstamped, dictionary) is not recorded.
    ///
    /// # Safety
    /// `obj` is a live ordinary `ObjectHeader`, and the caller proved from
    /// its shape's own key list (no accessor, no attribute on the key) that
    /// the name is own data at inline slot `slot`.
    pub(crate) unsafe fn prime(&self, obj: *const ObjectHeader, slot: u32) {
        if !crate::object::shapes::is_site_matchable_shape_id((*obj).parent_class_id) {
            return;
        }
        let word = std::ptr::read(obj as *const u64);
        if word == EMPTY {
            return;
        }
        let way = usize::from(self.next.get()) % WAYS;
        self.ways[way].set(Way { word, slot });
        self.next.set(((way + 1) % WAYS) as u8);
    }
}

/// The inherited form: on receivers whose word is `word` the name is absent
/// from the receiver's own key list and is an own plain DATA property at
/// inline slot `slot` of the `[[Prototype]]` the receiver's ShapeId names,
/// while that holder's word is `holder`.
///
/// The receiver's ShapeId pins its own key list and its prototype identity,
/// and the identity's word names the holder object wherever the collector
/// has moved it (`shapes::object_prototype_word`), so the memo holds no
/// pointer. The holder's word pins its key list, compared on every use.
#[derive(Clone, Copy)]
struct ProtoWay {
    key: i64,
    word: u64,
    holder: u64,
    slot: u32,
}

pub(crate) struct ProtoSlotMemo {
    ways: [Cell<ProtoWay>; WAYS],
    next: Cell<u8>,
}

impl ProtoSlotMemo {
    /// Only image descriptors and inline bytes have immutable, non-GC ids.
    /// A heap-string address can move or be reused for another name.
    #[inline]
    fn cacheable_key(key: i64) -> bool {
        matches!(
            key as u64 & crate::value::TAG_MASK,
            crate::string::STATIC_DISPATCH_TAG | crate::value::SHORT_STRING_TAG
        )
    }

    pub(crate) const fn new() -> Self {
        const W: ProtoWay = ProtoWay {
            key: 0,
            word: EMPTY,
            holder: EMPTY,
            slot: 0,
        };
        ProtoSlotMemo {
            ways: [Cell::new(W), Cell::new(W)],
            next: Cell::new(0),
        }
    }

    /// The holder and its inline slot for `obj`, when `obj`'s word was
    /// primed and the holder its shape names still carries the primed word.
    ///
    /// # Safety
    /// `obj` is a live, unforwarded ordinary `ObjectHeader`.
    #[inline]
    pub(crate) unsafe fn slot(
        &self,
        obj: *const ObjectHeader,
        key: i64,
    ) -> Option<(*const ObjectHeader, u32)> {
        if !Self::cacheable_key(key) {
            return None;
        }
        let word = std::ptr::read(obj as *const u64);
        let way = self
            .ways
            .iter()
            .map(Cell::get)
            .find(|way| way.word == word && way.key == key)?;
        let proto =
            crate::value::JSValue::from_bits(crate::object::shapes::object_prototype_word(obj));
        if !proto.is_pointer() {
            return None;
        }
        let holder = proto.as_pointer::<ObjectHeader>();
        (std::ptr::read(holder as *const u64) == way.holder).then_some((holder, way.slot))
    }

    /// Record that the memo's name is absent from `obj`'s own list and own
    /// plain data at inline slot `slot` of `holder`, the `[[Prototype]]`
    /// `obj`'s shape names. Receivers or holders whose stamp is not an
    /// ordinary-band ShapeId are not recorded.
    ///
    /// # Safety
    /// `obj` and `holder` are live ordinary objects, and the caller proved
    /// both facts from the two shapes' own key lists.
    pub(crate) unsafe fn prime(
        &self,
        obj: *const ObjectHeader,
        holder: *const ObjectHeader,
        key: i64,
        slot: u32,
    ) {
        if !Self::cacheable_key(key)
            || !crate::object::shapes::is_site_matchable_shape_id((*obj).parent_class_id)
            || !crate::object::shapes::is_site_matchable_shape_id((*holder).parent_class_id)
        {
            return;
        }
        let word = std::ptr::read(obj as *const u64);
        let way = usize::from(self.next.get()) % WAYS;
        self.ways[way].set(ProtoWay {
            key,
            word,
            holder: std::ptr::read(holder as *const u64),
            slot,
        });
        self.next.set(((way + 1) % WAYS) as u8);
    }
}

/// Ways of an [`AbsentKeyMemo`]: the few shapes one dictionary-like object
/// cycles through (an emitter's `_events` with and without one event).
const ABSENT_WAYS: usize = 4;

/// The negative form: receiver words whose own key list lacks the name. A
/// ShapeId names its key list, so absence is a fact of the word like
/// presence is; what the receiver INHERITS is not, and is the caller's to
/// judge (an emitter's `_events` has a null `[[Prototype]]`).
pub(crate) struct AbsentKeyMemo {
    words: [Cell<u64>; ABSENT_WAYS],
    next: Cell<u8>,
}

impl AbsentKeyMemo {
    pub(crate) const fn new() -> Self {
        AbsentKeyMemo {
            words: [
                Cell::new(EMPTY),
                Cell::new(EMPTY),
                Cell::new(EMPTY),
                Cell::new(EMPTY),
            ],
            next: Cell::new(0),
        }
    }

    /// Was `obj`'s word recorded as lacking the name?
    ///
    /// # Safety
    /// `obj` is a live, unforwarded ordinary `ObjectHeader`.
    #[inline(always)]
    pub(crate) unsafe fn absent(&self, obj: *const ObjectHeader) -> bool {
        let word = std::ptr::read(obj as *const u64);
        self.words.iter().any(|cell| cell.get() == word)
    }

    /// Record that `obj`'s own key list lacks the name.
    ///
    /// # Safety
    /// `obj` is a live ordinary `ObjectHeader`, and the caller proved the
    /// absence from its shape's own key list.
    pub(crate) unsafe fn prime(&self, obj: *const ObjectHeader) {
        if !crate::object::shapes::is_site_matchable_shape_id((*obj).parent_class_id) {
            return;
        }
        let word = std::ptr::read(obj as *const u64);
        let way = usize::from(self.next.get()) % ABSENT_WAYS;
        self.words[way].set(word);
        self.next.set(((way + 1) % ABSENT_WAYS) as u8);
    }
}

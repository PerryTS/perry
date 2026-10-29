//! The shape kind is shared by runtime births and compiled realm rows.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShapeObjectKind {
    /// An ordinary object whose own slots are proven plain data (a class
    /// instance, or a class-less receiver a birth site marked
    /// `OBJ_FLAG_PLAIN_ORDINARY`) and that carries no Array-subclass numeric
    /// proof. The ONE kind a store may be admitted on by its ShapeId alone
    /// (charter step 3, `shapes_store_kind`).
    Ordinary,
    Class,
    /// A function object (`GC_TYPE_CLOSURE`) whose own properties are exactly
    /// the intrinsic ones (`name`, `length`, `prototype`) and whose
    /// [[Prototype]] is the intrinsic its body kind names (the shape's
    /// `proto_id`). Minted in the exotic band: no per-site own-slot cache may
    /// hold it, because a closure's +16 is not an inline slot.
    Function,
    /// A function object that carries anything else (a user property, a
    /// deleted or redefined intrinsic, an accessor, a recorded
    /// [[Prototype]]): the answer lives on the object.
    FunctionDictionary,
    /// The ordinary layout of [`ShapeObjectKind::Ordinary`], on a receiver
    /// whose own slots are NOT proven plain data: a class-less receiver no
    /// birth site marked ordinary (`URL`, `Object.prototype`, a typed-array
    /// prototype, or a runtime-born record). Reads
    /// treat it as `Ordinary`; a store is never admitted on it by shape.
    OrdinaryUnmarked,
    /// [`ShapeObjectKind::Ordinary`] carrying the Array-subclass packed-numeric
    /// proof (`array::subclass`). Entered only by publishing the proof and
    /// left by retiring it (or by any other shape change), so the proof is a
    /// shape transition and a store word naming an `Ordinary` id can never
    /// match a proof-carrying receiver.
    OrdinaryNumericProof,
    /// A native-module namespace whose own keys and reads come from its
    /// export surface. The namespace brand is part of shape identity, so an
    /// ordinary record with a module name in a data slot cannot impersonate it.
    NativeNamespace,
    /// A bound call/apply adapter with the ordinary five bound slots plus a
    /// resolved function operand in slot 5. Appended to preserve every earlier
    /// ordinal and the packed kind field of existing shape records.
    FunctionBoundCall,
    FunctionBoundApply,
    /// Ordinary bound function: target, receiver and partial-argument slots.
    FunctionBound,
    /// Ordinary slots and links, plus receiver-local native fallback when
    /// an ordinary read produces undefined. Absence proofs must retain
    /// collecting native forwarding.
    OrdinaryNativeAlias,
    /// Ordinary ObjectHeader slots with a birth-declared [[Call]] method.
    /// Unlike Function kinds, this is not a ClosureHeader allocation.
    CallableOrdinary,
    /// Runtime-born ordinary storage with native read forwarding, without
    /// the plain-store admission carried by OrdinaryNativeAlias.
    OrdinaryUnmarkedNativeAlias,
}

impl ShapeObjectKind {
    #[inline]
    pub fn is_native_alias(self) -> bool {
        matches!(
            self,
            Self::OrdinaryNativeAlias | Self::OrdinaryUnmarkedNativeAlias
        )
    }

    #[inline]
    pub fn is_function_layout(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::FunctionBoundCall
                | Self::FunctionBoundApply
                | Self::FunctionBound
        )
    }

    /// A non-`GC_TYPE_OBJECT` receiver kind: minted in the exotic band.
    #[inline]
    pub fn is_exotic(self) -> bool {
        matches!(
            self,
            ShapeObjectKind::Function
                | ShapeObjectKind::FunctionDictionary
                | ShapeObjectKind::FunctionBoundCall
                | ShapeObjectKind::FunctionBoundApply
                | ShapeObjectKind::FunctionBound
        )
    }

    /// The ordinary object LAYOUT: every kind whose receiver reads exactly as
    /// an `Ordinary` one. What every layout / read consumer asks; only the
    /// store admission asks `== Ordinary`.
    #[inline]
    pub fn is_ordinary_layout(self) -> bool {
        matches!(
            self,
            ShapeObjectKind::Ordinary
                | ShapeObjectKind::OrdinaryUnmarked
                | ShapeObjectKind::OrdinaryNumericProof
                | ShapeObjectKind::NativeNamespace
                | ShapeObjectKind::OrdinaryNativeAlias
                | ShapeObjectKind::CallableOrdinary
                | ShapeObjectKind::OrdinaryUnmarkedNativeAlias
        )
    }

    /// The discriminant `facts_key` folds and `ShapeRecord` stores. Stable:
    /// it is written into a record field, so the values may not be reordered.
    #[inline]
    pub fn code(self) -> u64 {
        match self {
            ShapeObjectKind::Ordinary => 0,
            ShapeObjectKind::Class => 1,
            ShapeObjectKind::Function => 2,
            ShapeObjectKind::FunctionDictionary => 3,
            ShapeObjectKind::OrdinaryUnmarked => 4,
            ShapeObjectKind::OrdinaryNumericProof => 5,
            ShapeObjectKind::NativeNamespace => 6,
            ShapeObjectKind::FunctionBoundCall => 7,
            ShapeObjectKind::FunctionBoundApply => 8,
            ShapeObjectKind::FunctionBound => 9,
            ShapeObjectKind::OrdinaryNativeAlias => 10,
            ShapeObjectKind::CallableOrdinary => 11,
            ShapeObjectKind::OrdinaryUnmarkedNativeAlias => 12,
        }
    }
}

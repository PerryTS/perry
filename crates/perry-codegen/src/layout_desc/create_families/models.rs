//! The records this pass exposes to the rest of codegen: a prototype family at
//! one of its creation points (`Site`, `Create`, `Add`), the state machine that
//! follows a binding (`CreateState`), a holder read (`HolderRead`), and the
//! image key/entry facts a carried layout is made of (`ImageKey`, `ImageEntry`,
//! `ExportFact`). The pass's own working state (`Born`, `Link`, `KeyConst`,
//! `Analysis`, `ModuleCreateFamilies`) is deliberately not here.
use super::*;

/// One family layout.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    /// The family's carried descriptor.
    pub desc_global: String,
    /// Its keys, in order (slot = position).
    pub keys: Vec<String>,
    /// Its slot count (at least one, at least the key count).
    pub slots: u32,
    /// The `PerryProtoFamily` of its prototype binding.
    pub proto_global: String,
}

/// A `const a = Object.create(P)` creation point's family: born with one
/// slot for each key the statements after it store on `a`, in source order,
/// and one state per prefix of them (§3.2, as `layout_desc::FamilyAdd` for
/// literals).
#[derive(Clone, Debug)]
pub(crate) struct Create {
    /// `states[i]` holds `keys[..i]`; every state has `keys.len()` slots,
    /// and `states[0]` is the birth.
    pub states: Vec<String>,
    pub keys: Vec<String>,
    /// `P`'s binding.
    pub proto: u32,
    pub proto_global: String,
    /// The checked proof (empty when the census proves every add free): each
    /// prototype binding of `P`'s chain with its carried layout's
    /// descriptor, `P` first.
    pub chain: Vec<(u32, String)>,
    /// The chain ends at `%Object.prototype%` (else at null).
    pub object_root: bool,
}

/// One add of a [`Create`] family: its key `keys[index]`.
#[derive(Clone, Debug)]
pub(crate) struct Add {
    pub family: Create,
    pub index: usize,
}

/// The create families of the body being lowered (a field of its `FnCtx`):
/// the family of the `let` statement being lowered, until its
/// `Object.create` takes it, and the families of the bindings born so far.
#[derive(Clone, Debug, Default)]
pub(crate) struct CreateState {
    pub(super) pending: Option<(u32, Create)>,
    pub(super) born: HashMap<u32, Create>,
    /// The method call being lowered reads its method through
    /// [`holder_read`]: set around its dispatch, taken by the method site.
    pub(crate) method_holder: Option<HolderRead>,
}

/// `a.k` on a binding born of a family, read through the carried layouts
/// of its prototype chain (§3.6): `a` in one of the family's states (none
/// holds `k`) with no meta record, so it has no own `k` and its
/// [[Prototype]] is `P`; each prototype from `P` up to the first whose
/// carried layout holds `k` as data (the holder) still on that layout, so
/// none before it has `k` and the holder's `k` is its slot. One compare of a
/// layout word per object, no chain walked at run time.
#[derive(Clone, Debug)]
pub(crate) struct HolderRead {
    pub(super) states: Vec<String>,
    /// Each prototype binding from `P` to the holder, with its carried
    /// layout's descriptor.
    pub(super) chain: Vec<(u32, String)>,
    /// The holder's slot of `k`.
    pub(super) slot: u32,
}

/// A key of a carried prototype layout, as the runtime names it: a string,
/// a well-known symbol (its number in `perry_abi::WELL_KNOWN_SYMBOLS`), a
/// registered symbol (`Symbol.for` of the string) or the symbol a top-level
/// `const k = Symbol(…)` of this module binds (`Local(k)`: its value is
/// handed to the prototype's bind, [`lower_binding`]; never an export fact).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImageKey {
    Str(String),
    WellKnown(u32),
    Registered(String),
    Local(u32),
}

impl ImageKey {
    pub(super) fn is_symbol(&self) -> bool {
        !matches!(self, ImageKey::Str(_))
    }
}

/// Is a key fact meaningful in another module (not a binding of this one)?
pub(super) fn exportable(key: &ImageKey) -> bool {
    !matches!(key, ImageKey::Local(_))
}

/// One key of a carried prototype layout: the key and its attribute entry
/// (`perry_abi::LAYOUT_KEY_*`, the runtime's `key_attrs` entry bits).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageEntry {
    pub key: ImageKey,
    pub attrs: u8,
}

/// Attribute entry bits (`perry_abi`, pinned to the runtime's `key_attrs`).
pub(super) const NON_WRITABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_WRITABLE;
pub(super) const NON_ENUMERABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_ENUMERABLE;
pub(super) const NON_CONFIGURABLE: u8 = crate::runtime_abi::LAYOUT_KEY_NON_CONFIGURABLE;
pub(super) const ACCESSOR: u8 = crate::runtime_abi::LAYOUT_KEY_ACCESSOR;
pub(super) const HAS_GET: u8 = crate::runtime_abi::LAYOUT_KEY_HAS_GET;
pub(super) const HAS_SET: u8 = crate::runtime_abi::LAYOUT_KEY_HAS_SET;

/// Is the entry a writable data property (an add on a child shadows it)?
pub(super) fn plain_data(attrs: u8) -> bool {
    attrs & (ACCESSOR | NON_WRITABLE) == 0
}

/// What another module's export is at compile time: a property key (a
/// string constant or `Symbol.for` of one), or a literal's own keys (its
/// [`image`]). Keyed by (defining module path, export name).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ExportFact {
    Key(ImageKey),
    Image(Vec<ImageEntry>),
}

/// The program's export facts, by (module path, export name).
pub type ProtoExports = BTreeMap<(String, String), ExportFact>;

impl Site {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

impl Create {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

impl Add {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        self.family.form()
    }
}

impl HolderRead {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

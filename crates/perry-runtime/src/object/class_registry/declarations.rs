//! A class's ClassBody instance members as compiled: static image data.
//!
//! Codegen emits one constant [`ClassDeclaration`] per class into the image
//! (its method bodies, closure-convention entries, accessor halves, member
//! names and source order) and module init hands its address to
//! [`js_register_class_declaration`] once per class declaration. A native
//! class passes a Rust `static` the same way. Nothing here is written per
//! class evaluation: the image's index holds one link-time-constant address
//! per declared class id, so it is bounded by the program's class ids and
//! dies with the image.
//!
//! A computed member (`[k]() {}`) is a declaration whose name is unknown
//! until its class definition evaluates the key. Each evaluation names it on
//! the class holder (the class function object, `class_value_ptr`) as a
//! traced internal slot, overwriting the previous evaluation's name, so the
//! per-evaluation part lives on the holder and the declaration stays static.
//!
//! The decl prototype is born at most once per class per agent. Its members
//! are installed in definition order, and a birth that meets a computed
//! member whose name the definition has not evaluated yet stops there,
//! recording where on the holder; naming that member installs it and the
//! members after it up to the next unnamed one. ClassBody order holds even
//! though a computed registration must mint the holder (and so birth the
//! prototype) to have somewhere to keep the name.

use super::*;

/// [`ClassMemberDecl::kind`]: a method.
pub const CLASS_MEMBER_METHOD: u8 = 0;
/// [`ClassMemberDecl::kind`]: an accessor's getter half.
pub const CLASS_MEMBER_GETTER: u8 = 1;
/// [`ClassMemberDecl::kind`]: an accessor's setter half.
pub const CLASS_MEMBER_SETTER: u8 = 2;
/// [`ClassMemberDecl::flags`]: the method's last parameter is a synthesized
/// `arguments` object.
pub const CLASS_MEMBER_SYNTHETIC_ARGUMENTS: u8 = 1;
/// [`ClassMemberDecl::flags`]: the method has a trailing user rest parameter.
pub const CLASS_MEMBER_REST: u8 = 2;

/// One instance ClassBody member. Layout shared with codegen
/// (`{ ptr, ptr, ptr, i32, i32, i32, i8, i8, i16 }`, 40 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ClassMemberDecl {
    /// UTF-8 name bytes, or null for a computed key.
    pub name: *const u8,
    /// Method body `fn(this, args..) -> f64`, getter `fn(this) -> f64` or
    /// setter `fn(this, value) -> f64`.
    pub code: *const u8,
    /// A method's closure-convention entry (`<method>__eclo`'s
    /// `JsFunctionInfo`), or null.
    pub entry: *const u8,
    pub name_len: u32,
    /// The member's source-order token (its HIR function id); a computed
    /// member's registration names it by this token.
    pub definition_order: u32,
    /// A method's total parameter count (dispatch arity).
    pub param_count: u32,
    pub kind: u8,
    pub flags: u8,
    /// A setter's spec `.length`, or -1 when none was recorded.
    pub set_length: i16,
}

/// [`ClassDeclaration::flags`]: some member is an accessor half.
pub const CLASS_DECL_HAS_ACCESSORS: u32 = 1;
/// [`ClassDeclaration::flags`]: some member is computed (named per class
/// definition).
pub const CLASS_DECL_HAS_COMPUTED: u32 = 2;

/// A class's instance members, in definition order, the compiler's candidate
/// shape for its prototype birth (0 for none) and summary flags. Layout
/// shared with codegen (`{ ptr, i32, i32, i32, i32 }`).
#[repr(C)]
#[derive(Debug)]
pub struct ClassDeclaration {
    pub members: *const ClassMemberDecl,
    pub member_count: u32,
    pub birth_shape: u32,
    pub flags: u32,
    pub reserved: u32,
}

// SAFETY: image constants (or leaked test copies): never written after
// registration, and the pointers they hold name immutable code and bytes.
unsafe impl Sync for ClassMemberDecl {}
unsafe impl Send for ClassMemberDecl {}
unsafe impl Sync for ClassDeclaration {}
unsafe impl Send for ClassDeclaration {}

impl ClassDeclaration {
    /// A native class's declaration.
    pub const fn native(members: &'static [ClassMemberDecl]) -> Self {
        Self {
            members: members.as_ptr(),
            member_count: members.len() as u32,
            birth_shape: 0,
            flags: Self::flags_of(members),
            reserved: 0,
        }
    }

    /// The summary flags of `members`.
    pub const fn flags_of(members: &[ClassMemberDecl]) -> u32 {
        let mut flags = 0;
        let mut i = 0;
        while i < members.len() {
            let m = &members[i];
            if m.kind == CLASS_MEMBER_GETTER || m.kind == CLASS_MEMBER_SETTER {
                flags |= CLASS_DECL_HAS_ACCESSORS;
            }
            if m.name.is_null() {
                flags |= CLASS_DECL_HAS_COMPUTED;
            }
            i += 1;
        }
        flags
    }

    /// The members, in definition order.
    #[inline]
    pub fn members(&self) -> &[ClassMemberDecl] {
        if self.members.is_null() || self.member_count == 0 {
            return &[];
        }
        // SAFETY: `members` addresses `member_count` declarations that live
        // as long as the image.
        unsafe { std::slice::from_raw_parts(self.members, self.member_count as usize) }
    }

    /// Does any member wait for its class definition to name it?
    #[inline]
    pub(crate) fn has_computed_members(&self) -> bool {
        self.flags & CLASS_DECL_HAS_COMPUTED != 0
    }

    /// Does any member declare an accessor half?
    #[inline]
    pub(crate) fn has_accessors(&self) -> bool {
        self.flags & CLASS_DECL_HAS_ACCESSORS != 0
    }
}

impl ClassMemberDecl {
    /// A native class's method `name` running `code` (`fn(this, args..)`).
    pub const fn native_method(name: &'static str, code: *const u8, param_count: u32) -> Self {
        Self {
            name: name.as_ptr(),
            code,
            entry: std::ptr::null(),
            name_len: name.len() as u32,
            definition_order: 0,
            param_count,
            kind: CLASS_MEMBER_METHOD,
            flags: 0,
            set_length: -1,
        }
    }

    /// A native class's accessor half `name` running `code`.
    pub const fn native_accessor(name: &'static str, code: *const u8, setter: bool) -> Self {
        Self {
            name: name.as_ptr(),
            code,
            entry: std::ptr::null(),
            name_len: name.len() as u32,
            definition_order: 0,
            param_count: setter as u32,
            kind: if setter {
                CLASS_MEMBER_SETTER
            } else {
                CLASS_MEMBER_GETTER
            },
            flags: 0,
            set_length: if setter { 1 } else { -1 },
        }
    }

    /// The literal name's bytes, `None` for a computed member.
    #[inline]
    pub fn name_bytes(&self) -> Option<&[u8]> {
        if self.name.is_null() {
            return None;
        }
        // SAFETY: `name` addresses `name_len` bytes of the image.
        Some(unsafe { std::slice::from_raw_parts(self.name, self.name_len as usize) })
    }

    /// The literal name, `None` for a computed member (or one that is not
    /// UTF-8, which names no member).
    #[inline]
    pub fn literal_name(&self) -> Option<&str> {
        std::str::from_utf8(self.name_bytes()?).ok()
    }

    #[inline]
    pub fn is_method(&self) -> bool {
        self.kind == CLASS_MEMBER_METHOD
    }

    #[inline]
    pub fn is_accessor(&self) -> bool {
        self.kind == CLASS_MEMBER_GETTER || self.kind == CLASS_MEMBER_SETTER
    }

    #[inline]
    pub fn has_synthetic_arguments(&self) -> bool {
        self.flags & CLASS_MEMBER_SYNTHETIC_ARGUMENTS != 0
    }

    #[inline]
    pub fn has_rest(&self) -> bool {
        self.flags & CLASS_MEMBER_REST != 0
    }

    #[inline]
    pub fn set_length(&self) -> Option<u32> {
        u32::try_from(self.set_length).ok()
    }
}

/// Class ids below this index the image's dense array directly; the rest
/// (native and synthetic bands, test ids) live in its sorted spill.
const DENSE_DECLARATIONS: u32 = 1 << 16;

/// One image's class id -> declaration address index.
#[derive(Default)]
pub struct ClassDeclarationIndex {
    dense: Vec<usize>,
    spill: Vec<(u32, usize)>,
}

impl ClassDeclarationIndex {
    fn get(&self, class_id: u32) -> Option<&'static ClassDeclaration> {
        let addr = if class_id < DENSE_DECLARATIONS {
            self.dense.get(class_id as usize).copied().unwrap_or(0)
        } else {
            match self.spill.binary_search_by_key(&class_id, |e| e.0) {
                Ok(i) => self.spill[i].1,
                Err(_) => 0,
            }
        };
        // SAFETY: only registered declaration addresses are stored.
        (addr != 0).then(|| unsafe { &*(addr as *const ClassDeclaration) })
    }

    fn set(&mut self, class_id: u32, decl: &'static ClassDeclaration) {
        let addr = decl as *const ClassDeclaration as usize;
        if class_id < DENSE_DECLARATIONS {
            let i = class_id as usize;
            if self.dense.len() <= i {
                self.dense.resize(i + 1, 0);
            }
            self.dense[i] = addr;
        } else {
            match self.spill.binary_search_by_key(&class_id, |e| e.0) {
                Ok(i) => self.spill[i].1 = addr,
                Err(i) => self.spill.insert(i, (class_id, addr)),
            }
        }
    }

    /// Every registered declaration (census).
    pub(crate) fn declarations(&self) -> impl Iterator<Item = &'static ClassDeclaration> + '_ {
        self.dense
            .iter()
            .copied()
            .chain(self.spill.iter().map(|e| e.1))
            .filter(|&a| a != 0)
            // SAFETY: only registered declaration addresses are stored.
            .map(|a| unsafe { &*(a as *const ClassDeclaration) })
    }
}

/// The calling thread's image's declaration index (#8546).
pub static CLASS_DECLARATIONS: crate::object::class_image::ImageTable<
    std::sync::RwLock<ClassDeclarationIndex>,
> = crate::object::class_image::ImageTable::new(|image| &image.declarations);

/// The declaration of class `class_id`, if one was registered.
#[inline]
pub(crate) fn class_declaration(class_id: u32) -> Option<&'static ClassDeclaration> {
    if class_id == 0 {
        return None;
    }
    CLASS_DECLARATIONS.read().ok()?.get(class_id)
}

/// Register class `class_id`'s declaration: module init calls this once per
/// class declaration with the address of the class's image constant (a
/// native class with a Rust `static`).
///
/// # Safety
/// `decl` is null or addresses a [`ClassDeclaration`] that, with everything
/// it points at, lives as long as the image.
#[no_mangle]
pub unsafe extern "C" fn js_register_class_declaration(
    class_id: u32,
    decl: *const ClassDeclaration,
) {
    if class_id == 0 || decl.is_null() {
        return;
    }
    let decl: &'static ClassDeclaration = &*decl;
    if let Ok(mut index) = CLASS_DECLARATIONS.write() {
        index.set(class_id, decl);
    }
    declaration_registered(class_id, decl, &[]);
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REGISTER_CLASS_DECLARATION: unsafe extern "C" fn(u32, *const ClassDeclaration) =
    js_register_class_declaration;

/// The surface effects of a registered declaration: a class with a method
/// or a public accessor publishes its unbuilt holder, and a public accessor
/// reaches a prototype that already exists. `previous` is the declaration
/// this one replaces (tests extend a class member by member).
fn declaration_registered(class_id: u32, decl: &ClassDeclaration, previous: &[ClassMemberDecl]) {
    let mut publish = false;
    for member in decl.members() {
        let Some(name) = member.literal_name() else {
            continue;
        };
        if member.is_method() {
            publish = true;
        } else if !name.starts_with('#') {
            publish = true;
        }
    }
    if publish {
        super::registration::publish_unbuilt_holder(class_id);
    }
    let mut noted: Vec<&str> = Vec::new();
    for member in decl.members() {
        let Some(name) = member.literal_name() else {
            continue;
        };
        if !member.is_accessor() || name.starts_with('#') || noted.contains(&name) {
            continue;
        }
        noted.push(name);
        let newly_declared = !previous
            .iter()
            .any(|p| p.is_accessor() && p.literal_name() == Some(name));
        super::decl_accessors::note_instance_accessor_registered(class_id, name, newly_declared);
    }
}

// ---------------------------------------------------------------------------
// Evaluated names of computed members: traced slots on the class holder.
// ---------------------------------------------------------------------------

/// The holder-internal key of the name the latest evaluation gave the
/// computed member with source-order token `order` (see
/// [`super::state::class_declaration_value_key`] for the namespace).
fn computed_name_key(order: u32) -> String {
    format!("\u{1}k:{order}")
}

/// The holder-internal key of the birth's resume position: the index of the
/// first member the decl prototype does not hold yet because an earlier
/// computed member had no name when it was born.
const RESUME_KEY: &str = "\u{1}r";

/// What a member is called on its class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemberName {
    /// A string-keyed member.
    Str(String),
    /// A computed member whose key evaluated to a symbol: not a string
    /// member (its own tables hold it).
    NotAString,
    /// A computed member whose key the definition has not evaluated.
    Unnamed,
}

/// The internal own key of a per-evaluation class object holding the names
/// its definition gave the class's computed members: an array with one
/// element per computed member, in declaration order (a string, `null` for a
/// non-string key, `undefined` before the definition reached it).
pub(crate) const CLASS_COMPUTED_NAMES_KEY: &[u8] = b"#<perry:class-computed-names>";

crate::perry_thread_local! {
    /// The computed-member names of the evaluation whose prototype this
    /// thread is building, innermost last: `(class id, names by ordinal)`.
    /// Plain Rust data copied off the evaluation's class object, so it holds
    /// no heap pointer.
    static EVALUATION_NAMES: std::cell::RefCell<Vec<(u32, Vec<MemberName>)>> =
        std::cell::RefCell::new(Vec::new());
}

/// Pops the names [`enter_evaluation_names`] pushed.
pub(crate) struct EvaluationNamesScope(());

impl Drop for EvaluationNamesScope {
    fn drop(&mut self) {
        EVALUATION_NAMES.with(|names| {
            names.borrow_mut().pop();
        });
    }
}

/// While the returned scope lives, the computed members of `class_id` are
/// called what evaluation class object `class` named them. `None` (nothing
/// entered) when the evaluation keeps no names: its definition named them on
/// the class holder.
///
/// # Safety
/// `class` is a live class object.
pub(crate) unsafe fn enter_evaluation_names(
    class: *const ObjectHeader,
    class_id: u32,
) -> Option<EvaluationNamesScope> {
    if !class_has_computed_members(class_id) {
        return None;
    }
    let names = super::class_object_own_field_bytes(class, CLASS_COMPUTED_NAMES_KEY)?;
    let names = crate::JSValue::from_bits(names.to_bits());
    if !names.is_pointer() {
        return None;
    }
    let arr = names.as_pointer::<crate::ArrayHeader>();
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(member_name_of_value(crate::array::js_array_get_f64(arr, i)));
    }
    EVALUATION_NAMES.with(|names| names.borrow_mut().push((class_id, out)));
    Some(EvaluationNamesScope(()))
}

/// The [`MemberName`] a kept name value stands for.
fn member_name_of_value(value: f64) -> MemberName {
    let js = crate::JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return MemberName::Unnamed;
    }
    if !js.is_any_string() {
        return MemberName::NotAString;
    }
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // SAFETY: a string value read from a traced slot.
    match unsafe { crate::string::js_string_key_bytes(js, &mut buf) } {
        Some(bytes) => MemberName::Str(String::from_utf8_lossy(bytes).into_owned()),
        None => MemberName::NotAString,
    }
}

/// The value kept for `name`.
fn member_name_value(name: &MemberName) -> f64 {
    match name {
        MemberName::Str(s) => {
            let _no_move = crate::gc::GcSuppressScope::new();
            let string = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(crate::value::JSValue::string_ptr(string).bits())
        }
        MemberName::NotAString => f64::from_bits(crate::value::TAG_NULL),
        MemberName::Unnamed => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// The ordinal of computed member `member` among `decl`'s computed members.
fn computed_ordinal(decl: &ClassDeclaration, member: &ClassMemberDecl) -> Option<usize> {
    decl.members()
        .iter()
        .filter(|m| m.name.is_null())
        .position(|m| std::ptr::eq(m, member))
}

/// The evaluated name of computed member `member` of class `class_id`.
fn computed_member_name(class_id: u32, member: &ClassMemberDecl) -> MemberName {
    let scoped = EVALUATION_NAMES.with(|names| {
        let names = names.borrow();
        let (scope_class, scope_names) = names.last()?;
        if *scope_class != class_id {
            return None;
        }
        let ordinal = computed_ordinal(class_declaration(class_id)?, member)?;
        Some(
            scope_names
                .get(ordinal)
                .cloned()
                .unwrap_or(MemberName::Unnamed),
        )
    });
    if let Some(name) = scoped {
        return name;
    }
    let Some(holder) = crate::object::class_value::class_value_if_minted(class_id) else {
        return MemberName::Unnamed;
    };
    // SAFETY: this agent's live class function object.
    let Some(value) = (unsafe {
        crate::closure::props::state_internal_get(
            holder as usize,
            &computed_name_key(member.definition_order),
        )
    }) else {
        return MemberName::Unnamed;
    };
    member_name_of_value(value)
}

/// The name of `member` of class `class_id`.
pub(crate) fn class_member_name(class_id: u32, member: &ClassMemberDecl) -> MemberName {
    match member.name_bytes() {
        Some(bytes) => match std::str::from_utf8(bytes) {
            Ok(name) => MemberName::Str(name.to_string()),
            Err(_) => MemberName::NotAString,
        },
        None => computed_member_name(class_id, member),
    }
}

/// Does `member` of `class_id` have the name `name`?
#[inline]
fn member_is_named(class_id: u32, member: &ClassMemberDecl, name: &str) -> bool {
    match member.name_bytes() {
        Some(literal) => literal == name.as_bytes(),
        None => matches!(computed_member_name(class_id, member), MemberName::Str(s) if s == name),
    }
}

/// The index (in declaration order) of the method `name` of class
/// `class_id` as last declared, and the declaration's member count.
pub(crate) fn class_method_decl_index(class_id: u32, name: &str) -> Option<(usize, usize)> {
    let decl = class_declaration(class_id)?;
    let members = decl.members();
    members
        .iter()
        .rposition(|m| m.is_method() && member_is_named(class_id, m, name))
        .map(|index| (index, members.len()))
}

/// The method `name` of class `class_id` as last declared.
pub(crate) fn class_method_decl(class_id: u32, name: &str) -> Option<&'static ClassMemberDecl> {
    let decl = class_declaration(class_id)?;
    decl.members()
        .iter()
        .rev()
        .find(|m| m.is_method() && member_is_named(class_id, m, name))
}

/// The accessor `name` of class `class_id` as declared: each half from the
/// last member declaring it, and the setter's spec `.length`.
pub(crate) fn class_accessor_decl(class_id: u32, name: &str) -> Option<AccessorDecl> {
    let decl = class_declaration(class_id)?;
    if !decl.has_accessors() {
        return None;
    }
    let mut out = AccessorDecl::default();
    let mut any = false;
    for member in decl.members() {
        if !member.is_accessor()
            || member.code.is_null()
            || !member_is_named(class_id, member, name)
        {
            continue;
        }
        any = true;
        if member.kind == CLASS_MEMBER_SETTER {
            out.set = member.code as usize;
            out.set_length = member.set_length();
        } else {
            out.get = member.code as usize;
        }
    }
    any.then_some(out)
}

/// Index of the first member of `decl` the definition has not named.
fn first_unnamed(class_id: u32, decl: &ClassDeclaration) -> Option<usize> {
    if !decl.has_computed_members() {
        return None;
    }
    decl.members()
        .iter()
        .position(|m| m.name.is_null() && computed_member_name(class_id, m) == MemberName::Unnamed)
}

/// The string-keyed members of class `class_id` the decl prototype holds
/// (or would hold if born now), each once per kind in declaration order:
/// `(methods, accessors)`. Members from the first one whose computed key the
/// definition has not evaluated onward are left out.
pub(crate) fn class_declared_member_names(class_id: u32) -> (Vec<String>, Vec<String>) {
    let mut methods: Vec<String> = Vec::new();
    let mut accessors: Vec<String> = Vec::new();
    let Some(decl) = class_declaration(class_id) else {
        return (methods, accessors);
    };
    let end = first_unnamed(class_id, decl).unwrap_or(decl.members().len());
    for member in &decl.members()[..end] {
        let MemberName::Str(name) = class_member_name(class_id, member) else {
            continue;
        };
        let list = if member.is_method() {
            &mut methods
        } else {
            &mut accessors
        };
        if !list.contains(&name) {
            list.push(name);
        }
    }
    (methods, accessors)
}

/// The method of class `class_id` whose closure-convention entry is `entry`,
/// with its name.
pub(crate) fn class_method_decl_by_entry(
    class_id: u32,
    entry: usize,
) -> Option<(String, &'static ClassMemberDecl)> {
    let decl = class_declaration(class_id)?;
    decl.members().iter().rev().find_map(|m| {
        if !m.is_method() || m.entry as usize != entry {
            return None;
        }
        match class_member_name(class_id, m) {
            MemberName::Str(name) => Some((name, m)),
            _ => None,
        }
    })
}

/// The compiler's candidate prototype shape of class `class_id` (0: none).
pub(crate) fn class_birth_shape(class_id: u32) -> u32 {
    class_declaration(class_id).map_or(0, |d| d.birth_shape)
}

/// Does class `class_id` declare a computed instance member?
pub(crate) fn class_has_computed_members(class_id: u32) -> bool {
    class_declaration(class_id).is_some_and(ClassDeclaration::has_computed_members)
}

/// After a decl prototype birth of class `class_id`: when a computed member
/// still had no name, record where installation resumes.
pub(crate) fn note_decl_prototype_born(class_id: u32) {
    let Some(decl) = class_declaration(class_id) else {
        return;
    };
    let Some(stop) = first_unnamed(class_id, decl) else {
        return;
    };
    let Some(holder) = crate::object::class_value::class_value_if_minted(class_id) else {
        return;
    };
    // SAFETY: this agent's live class function object.
    unsafe {
        crate::closure::props::state_internal_set(holder as usize, RESUME_KEY, stop as f64);
    }
}

/// The class definition evaluated computed member `definition_order` of class
/// `class_id` to `name`: keep the name on the holder (minting it), and, when
/// the decl prototype stopped at this member, install it and the members
/// after it up to the next unnamed one.
///
/// A later evaluation of the same declaration renames the member for the
/// evaluations built after it; the decl prototype, which belongs to the
/// evaluation that built it, is not touched again.
pub(crate) fn name_computed_member(class_id: u32, definition_order: u32, name: MemberName) {
    let Some(decl) = class_declaration(class_id) else {
        return;
    };
    let Some(index) = decl
        .members()
        .iter()
        .position(|m| m.name.is_null() && m.definition_order == definition_order)
    else {
        return;
    };
    let holder = crate::object::class_value::class_value_ptr(class_id) as usize;
    let previous = computed_member_name(class_id, &decl.members()[index]);
    let key = computed_name_key(definition_order);
    let value = member_name_value(&match name {
        MemberName::Unnamed => MemberName::NotAString,
        ref other => other.clone(),
    });
    // SAFETY: this agent's live class function object.
    unsafe { crate::closure::props::state_internal_set(holder, &key, value) };
    if previous != name && previous != MemberName::Unnamed {
        // The method value kept for the old name described this member under
        // that name; the new one is materialized on demand.
        super::state::class_declaration_value_forget(class_id, index);
    }
    // SAFETY: as above.
    let resume = unsafe { crate::closure::props::state_internal_get(holder, RESUME_KEY) };
    let Some(resume) = resume.map(|r| r as usize) else {
        return;
    };
    if resume != index {
        return;
    }
    let proto = super::state::class_decl_prototype_object(class_id);
    let next = first_unnamed(class_id, decl);
    // SAFETY: as above.
    unsafe {
        match next {
            Some(stop) => {
                crate::closure::props::state_internal_set(holder, RESUME_KEY, stop as f64)
            }
            None => {
                crate::closure::props::state_internal_remove(holder, RESUME_KEY);
            }
        }
    }
    if proto.is_null() {
        return;
    }
    let end = next.unwrap_or(decl.members().len());
    // The string members the prototype holds (a symbol method's dispatch
    // alias and a re-added deleted key are not among them).
    let members: Vec<String> = super::state::class_prototype_member_names(class_id)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_raw_mut_ptr(proto);
    let mut installed: Vec<String> = Vec::new();
    for member in &decl.members()[index..end] {
        let MemberName::Str(name) = class_member_name(class_id, member) else {
            continue;
        };
        if !members.contains(&name) || installed.contains(&name) {
            continue;
        }
        let accessor = member.is_accessor();
        proto_h.with_mut_ptr(|proto: *mut ObjectHeader| {
            if accessor {
                super::decl_accessors::install_decl_prototype_accessor(proto, class_id, &name);
            } else {
                super::state::install_class_decl_prototype_method_field(proto, class_id, &name);
            }
        });
        installed.push(name);
    }
}

/// A class EXPRESSION's definition evaluated computed member
/// `definition_order` of template `class_id` to `name` for evaluation class
/// object `owner`: the name is the evaluation's own, kept on its class
/// object, which builds its prototype from it. The template's holder is not
/// touched (no evaluation reads the template's own prototype).
///
/// # Safety
/// `owner` is a live per-evaluation class object of template `class_id`.
pub(crate) unsafe fn name_evaluation_computed_member(
    owner: f64,
    class_id: u32,
    definition_order: u32,
    name: MemberName,
) {
    let Some(decl) = class_declaration(class_id) else {
        return;
    };
    let Some(member) = decl
        .members()
        .iter()
        .find(|m| m.name.is_null() && m.definition_order == definition_order)
    else {
        return;
    };
    let Some(ordinal) = computed_ordinal(decl, member) else {
        return;
    };
    let count = decl.members().iter().filter(|m| m.name.is_null()).count();
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(owner);
    let obj = || {
        crate::JSValue::from_bits(owner.get_nanbox_f64().to_bits()).as_pointer::<ObjectHeader>()
            as *mut ObjectHeader
    };
    let existing = super::class_object_own_field_bytes(obj(), CLASS_COMPUTED_NAMES_KEY)
        .filter(|v| crate::JSValue::from_bits(v.to_bits()).is_pointer());
    let names = match existing {
        Some(v) => scope.root_nanbox_f64(v),
        None => {
            let mut arr = crate::array::js_array_alloc(count as u32);
            for _ in 0..count {
                arr = crate::array::js_array_push_f64(
                    arr,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                );
            }
            let names = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(arr as i64));
            crate::object::field_get_set::class_object_add_internal_for(
                obj(),
                crate::object::field_get_set::InternalKey::ComputedNames,
                names.get_nanbox_f64(),
            );
            names
        }
    };
    let value = scope.root_nanbox_f64(member_name_value(&name));
    let arr = crate::JSValue::from_bits(names.get_nanbox_f64().to_bits())
        .as_pointer::<crate::ArrayHeader>() as *mut crate::ArrayHeader;
    crate::array::js_array_set_f64(arr, ordinal as u32, value.get_nanbox_f64());
}

// ---------------------------------------------------------------------------
// Unit tests build their classes member by member through these: each call
// leaks a copy of the class's declaration with one member appended.
// ---------------------------------------------------------------------------

#[cfg(test)]
fn test_counter() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1 << 30);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Append `member` (its name copied and leaked) to class `class_id`'s
/// declaration and re-register it.
#[cfg(test)]
pub(crate) fn test_declare_member(class_id: u32, name: &[u8], member: ClassMemberDecl) {
    test_declare_member_with(class_id, name, member, true);
}

/// [`test_declare_member`] without the registration's surface effects (the
/// bare declaration record, as a test that seeds metadata directly needs).
#[cfg(test)]
pub(crate) fn test_declare_member_quiet(class_id: u32, name: &[u8], member: ClassMemberDecl) {
    test_declare_member_with(class_id, name, member, false);
}

#[cfg(test)]
fn test_declare_member_with(
    class_id: u32,
    name: &[u8],
    mut member: ClassMemberDecl,
    surface: bool,
) {
    if class_id == 0 {
        return;
    }
    let previous: Vec<ClassMemberDecl> = class_declaration(class_id)
        .map(|d| d.members().to_vec())
        .unwrap_or_default();
    let birth_shape = class_birth_shape(class_id);
    let name: &'static [u8] = Box::leak(name.to_vec().into_boxed_slice());
    member.name = name.as_ptr();
    member.name_len = name.len() as u32;
    if member.definition_order == 0 {
        member.definition_order = test_counter();
    }
    let mut members = previous.clone();
    members.push(member);
    let members: &'static [ClassMemberDecl] = Box::leak(members.into_boxed_slice());
    let decl: &'static ClassDeclaration = Box::leak(Box::new(ClassDeclaration {
        members: members.as_ptr(),
        member_count: members.len() as u32,
        birth_shape,
        flags: ClassDeclaration::flags_of(members),
        reserved: 0,
    }));
    if let Ok(mut index) = CLASS_DECLARATIONS.write() {
        index.set(class_id, decl);
    }
    if surface {
        declaration_registered(class_id, decl, &previous);
    }
}

/// Append a computed member (no name; its definition names it by
/// `definition_order`) to class `class_id`'s declaration.
#[cfg(test)]
pub(crate) fn test_declare_computed_member(
    class_id: u32,
    definition_order: u32,
    mut member: ClassMemberDecl,
) {
    let previous: Vec<ClassMemberDecl> = class_declaration(class_id)
        .map(|d| d.members().to_vec())
        .unwrap_or_default();
    member.name = std::ptr::null();
    member.name_len = 0;
    member.definition_order = definition_order;
    let mut members = previous;
    members.push(member);
    let members: &'static [ClassMemberDecl] = Box::leak(members.into_boxed_slice());
    let decl: &'static ClassDeclaration = Box::leak(Box::new(ClassDeclaration {
        members: members.as_ptr(),
        member_count: members.len() as u32,
        birth_shape: class_birth_shape(class_id),
        flags: ClassDeclaration::flags_of(members),
        reserved: 0,
    }));
    if let Ok(mut index) = CLASS_DECLARATIONS.write() {
        index.set(class_id, decl);
    }
}

/// Set class `class_id`'s compiler birth-shape candidate.
#[cfg(test)]
pub(crate) fn test_declare_birth_shape(class_id: u32, shape_id: u32) {
    let previous: Vec<ClassMemberDecl> = class_declaration(class_id)
        .map(|d| d.members().to_vec())
        .unwrap_or_default();
    let members: &'static [ClassMemberDecl] = Box::leak(previous.into_boxed_slice());
    let decl: &'static ClassDeclaration = Box::leak(Box::new(ClassDeclaration {
        members: members.as_ptr(),
        member_count: members.len() as u32,
        birth_shape: shape_id,
        flags: ClassDeclaration::flags_of(members),
        reserved: 0,
    }));
    if let Ok(mut index) = CLASS_DECLARATIONS.write() {
        index.set(class_id, decl);
    }
}

/// Give the last method `name` of class `class_id` the closure-convention
/// entry `entry`.
#[cfg(test)]
pub(crate) fn test_set_method_entry(class_id: u32, name: &str, entry: usize) {
    let Some(decl) = class_declaration(class_id) else {
        return;
    };
    let mut members = decl.members().to_vec();
    let Some(member) = members
        .iter_mut()
        .rev()
        .find(|m| m.is_method() && m.literal_name() == Some(name))
    else {
        return;
    };
    member.entry = entry as *const u8;
    let members: &'static [ClassMemberDecl] = Box::leak(members.into_boxed_slice());
    let decl: &'static ClassDeclaration = Box::leak(Box::new(ClassDeclaration {
        members: members.as_ptr(),
        member_count: members.len() as u32,
        birth_shape: decl.birth_shape,
        flags: ClassDeclaration::flags_of(members),
        reserved: 0,
    }));
    if let Ok(mut index) = CLASS_DECLARATIONS.write() {
        index.set(class_id, decl);
    }
}

/// A method member with the given body facts.
#[cfg(test)]
pub(crate) fn test_method_member(
    code: usize,
    param_count: u32,
    synthetic: bool,
    rest: bool,
    entry: usize,
) -> ClassMemberDecl {
    ClassMemberDecl {
        name: std::ptr::null(),
        code: code as *const u8,
        entry: entry as *const u8,
        name_len: 0,
        definition_order: 0,
        param_count,
        kind: CLASS_MEMBER_METHOD,
        flags: if synthetic {
            CLASS_MEMBER_SYNTHETIC_ARGUMENTS
        } else {
            0
        } | if rest { CLASS_MEMBER_REST } else { 0 },
        set_length: -1,
    }
}

/// An accessor half member.
#[cfg(test)]
pub(crate) fn test_accessor_member(
    code: usize,
    setter: bool,
    set_length: Option<u32>,
) -> ClassMemberDecl {
    ClassMemberDecl {
        name: std::ptr::null(),
        code: code as *const u8,
        entry: std::ptr::null(),
        name_len: 0,
        definition_order: 0,
        param_count: u32::from(setter),
        kind: if setter {
            CLASS_MEMBER_SETTER
        } else {
            CLASS_MEMBER_GETTER
        },
        flags: 0,
        set_length: set_length.map_or(-1, |l| l as i16),
    }
}

#[cfg(test)]
#[path = "declarations_tests.rs"]
mod tests;

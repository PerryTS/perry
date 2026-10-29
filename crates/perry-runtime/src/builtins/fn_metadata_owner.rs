//! Canonical metadata ownership. Startup publishes an image descriptor, not
//! function-address keys. Closure operations index that owner directly.
use crate::codegen_abi::{FunctionMetadataIdentity, FunctionMetadataOwner, JsFunctionInfo};
use std::sync::{atomic::Ordering, Arc, Mutex, OnceLock};

#[derive(Default)]
struct Overrides {
    name: Option<Arc<[u8]>>,
    source: Option<(Arc<[u8]>, bool)>,
}
struct CopiedEntry {
    code: usize,
    name: Option<Arc<[u8]>>,
    source: Option<Arc<[u8]>>,
    ordinary: bool,
}
/// Payload belongs to its descriptor. It contains no GC-managed pointers.
/// Overrides are sparse, ordered by ordinal and allocated only on a write.
struct OwnerState {
    copied: OnceLock<Vec<CopiedEntry>>,
    overrides: Mutex<Vec<(u32, Overrides)>>,
    /// The owner's registration index plus one (0: not registered): the
    /// order module init published it in, a deterministic owner identity
    /// that a ConstFn body's shape fold reads without the owners lock.
    registration: std::sync::atomic::AtomicU32,
}
impl OwnerState {
    fn new(copied: Option<Vec<CopiedEntry>>) -> Self {
        let storage = OnceLock::new();
        if let Some(entries) = copied {
            let _ = storage.set(entries);
        }
        Self {
            copied: storage,
            overrides: Mutex::new(Vec::new()),
            registration: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[derive(Clone, Copy)]
enum RegisteredOwner {
    Image(&'static FunctionMetadataOwner),
    Copied(&'static OwnerState),
}
fn owners() -> &'static Mutex<Vec<RegisteredOwner>> {
    static OWNERS: OnceLock<Mutex<Vec<RegisteredOwner>>> = OnceLock::new();
    crate::once_init::get_or_init(&OWNERS, || Mutex::new(Vec::new()))
}
fn image_info(owner: &FunctionMetadataOwner, ordinal: usize) -> Option<&JsFunctionInfo> {
    if ordinal >= owner.count as usize || owner.entries.is_null() {
        return None;
    }
    // SAFETY: publication validates the descriptor's lifetime; each relative
    // entry names a complete info record emitted with this owner.
    unsafe {
        let delta = owner.entries.add(ordinal).read();
        let address = (owner.entries as usize).wrapping_add_signed(delta as isize);
        (address as *const JsFunctionInfo).as_ref()
    }
}
/// The owner's `state` word: null, an `OwnerState` (aligned, low bit clear),
/// or, while no payload exists, the owner's registration index plus one
/// shifted over a set low bit. Registration therefore allocates nothing, and
/// the index stays on the owner record (CKI S2).
const STATE_INDEX_TAG: usize = 1;
fn state(owner: &FunctionMetadataOwner) -> Option<&'static OwnerState> {
    let word = owner.state.load(Ordering::Acquire);
    if word as usize & STATE_INDEX_TAG != 0 {
        return None;
    }
    unsafe { word.cast::<OwnerState>().as_ref() }
}
fn install_state(owner: &FunctionMetadataOwner, value: OwnerState) -> &'static OwnerState {
    let pointer = Box::into_raw(Box::new(value));
    let mut current = owner.state.load(Ordering::Acquire);
    loop {
        if !current.is_null() && current as usize & STATE_INDEX_TAG == 0 {
            // Another caller installed the payload first.
            unsafe { drop(Box::from_raw(pointer)) };
            return unsafe { &*current.cast::<OwnerState>() };
        }
        // The payload inherits a registration index the word already holds.
        let index = if current.is_null() { 0 } else { (current as usize >> 1) as u32 };
        unsafe { (*pointer).registration.store(index, Ordering::Relaxed) };
        match owner.state.compare_exchange(
            current,
            pointer.cast(),
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return unsafe { &*pointer },
            Err(seen) => current = seen,
        }
    }
}
/// Record `owner`'s registration index plus one, on the owner itself.
fn set_registration(owner: &FunctionMetadataOwner, index_plus_one: u32) {
    let tagged = ((index_plus_one as usize) << 1 | STATE_INDEX_TAG) as *mut std::ffi::c_void;
    let mut current = owner.state.load(Ordering::Acquire);
    loop {
        if !current.is_null() && current as usize & STATE_INDEX_TAG == 0 {
            let st = unsafe { &*current.cast::<OwnerState>() };
            st.registration.store(index_plus_one, Ordering::Release);
            return;
        }
        match owner.state.compare_exchange(current, tagged, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return,
            Err(seen) => current = seen,
        }
    }
}
/// `owner`'s registration index plus one, or 0 before registration.
fn registration(owner: &FunctionMetadataOwner) -> u32 {
    let word = owner.state.load(Ordering::Acquire);
    if word as usize & STATE_INDEX_TAG != 0 {
        return (word as usize >> 1) as u32;
    }
    // SAFETY: an untagged non-null word is this owner's payload.
    unsafe { word.cast::<OwnerState>().as_ref() }
        .map_or(0, |st| st.registration.load(Ordering::Acquire))
}

/// Register one image's canonical owner. All entry ordinals and info ranges
/// already exist. Permanent publication does no address-keyed name work.
///
/// # Safety
/// Owner and its relative entries/infos must be valid for this call and while
/// its function objects remain live. A nonzero `permanent` promises process
/// lifetime; otherwise the canonical owner copies bytes before image unload.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_metadata(
    owner: *const FunctionMetadataOwner,
    permanent: i32,
) {
    let Some(owner) = owner.as_ref() else {
        return;
    };
    if owner.count == 0 {
        return;
    }
    let registered = if permanent != 0 {
        RegisteredOwner::Image(owner)
    } else {
        // A cyclic importer may infer a name before this module publishes.
        // Preserve those overrides and still establish copied image custody.
        let owned = state(owner).unwrap_or_else(|| install_state(owner, OwnerState::new(None)));
        owned.copied.get_or_init(|| {
            (0..owner.count as usize)
                .map(|ordinal| {
                    let info = image_info(owner, ordinal).expect("metadata info entry");
                    CopiedEntry {
                        code: info.code as usize,
                        name: super::fn_metadata::info_name_bytes(info).map(Arc::from),
                        source: super::fn_metadata::retained_source_for_info(info)
                            .map(|s| Arc::from(s.as_bytes())),
                        ordinary: info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0,
                    }
                })
                .collect()
        });
        RegisteredOwner::Copied(owned)
    };
    if let Ok(mut list) = owners().lock() {
        // Module init may be repeated by another agent. Compare owners only;
        // no code address participates and no per-function entry is inserted.
        let index = match list.iter().position(|item| match (item, registered) {
            (RegisteredOwner::Image(a), RegisteredOwner::Image(b)) => std::ptr::eq(*a, b),
            (RegisteredOwner::Copied(a), RegisteredOwner::Copied(b)) => std::ptr::eq(*a, b),
            _ => false,
        }) {
            Some(index) => index,
            None => {
                list.push(registered);
                list.len() - 1
            }
        };
        // The owner record carries its own index (CKI S2).
        set_registration(owner, index as u32 + 1);
    }
}

/// A ConstFn body's word in the shape-identity fold (CKI S2): its owner's
/// registration index (plus one) and its entry ordinal. Program order fixes
/// both, so the word is the same wherever the link put the image; the info's
/// own address never enters. An info without the metadata tail, or one whose
/// owner is not registered, folds its ordinal alone (or 0): the fold is a
/// bucket key, and the exact facts compare still tells such bodies apart.
pub(crate) fn info_identity_word(info: u64) -> u64 {
    // SAFETY: a ConstFn slot names a `JsFunctionInfo` its image keeps alive
    // (`checked_constfn_static_entries`, the key-add admission).
    let Some(info) = (unsafe { (info as usize as *const JsFunctionInfo).as_ref() }) else {
        return 0;
    };
    let Some(FunctionMetadataIdentity { owner, ordinal }) = info.metadata_identity() else {
        return 0;
    };
    // SAFETY: `metadata_identity` validated the owner pointer and ordinal.
    let index = registration(unsafe { &*owner });
    (u64::from(index) << 32) | u64::from(ordinal)
}

/// `count` ConstFn info words of distinct registered owners, for tests that
/// fold bodies into shape identity: a ConstFn slot names a live info.
#[cfg(test)]
pub(crate) fn test_registered_infos(count: usize) -> Vec<u64> {
    (0..count)
        .map(|i| {
            let owner: &'static FunctionMetadataOwner =
                Box::leak(Box::new(FunctionMetadataOwner::native()));
            // SAFETY: a leaked, permanent native owner.
            unsafe { js_register_function_metadata(owner, 1) };
            let info: &'static crate::codegen_abi::NativeFunctionInfo =
                Box::leak(Box::new(crate::codegen_abi::NativeFunctionInfo::new(
                    // SAFETY: a test code word; nothing calls it.
                    unsafe { JsFunctionInfo::from_code((0x4c00_0000 + i * 64) as *const u8, 0) },
                    owner,
                )));
            &info.info as *const JsFunctionInfo as usize as u64
        })
        .collect()
}

/// A borrowed canonical record. The info's flag licenses its owner tail.
#[derive(Clone, Copy)]
pub(super) struct Record<'a> {
    owner: &'a FunctionMetadataOwner,
    ordinal: u32,
}
impl<'a> Record<'a> {
    pub(super) fn for_info(info: &'a JsFunctionInfo) -> Option<Self> {
        let FunctionMetadataIdentity { owner, ordinal } = info.metadata_identity()?;
        Some(Self {
            owner: unsafe { &*owner },
            ordinal,
        })
    }
    fn image_info(self) -> Option<&'a JsFunctionInfo> {
        image_info(self.owner, self.ordinal as usize)
    }
    fn copied(self) -> Option<&'static CopiedEntry> {
        state(self.owner)?.copied.get()?.get(self.ordinal as usize)
    }
    pub(super) fn with_name<R>(self, read: impl FnOnce(&str) -> R) -> Option<R> {
        if let Some(state) = state(self.owner) {
            if let Ok(slots) = state.overrides.lock() {
                if let Ok(index) = slots.binary_search_by_key(&self.ordinal, |s| s.0) {
                    if let Some(name) = slots[index]
                        .1
                        .name
                        .as_deref()
                        .and_then(super::fn_metadata::decode_registered_bytes)
                    {
                        return Some(read(name));
                    }
                }
            }
        }
        let bytes = if let Some(copy) = self.copied() {
            copy.name.as_deref()
        } else {
            self.image_info()
                .and_then(super::fn_metadata::info_name_bytes)
        };
        bytes
            .and_then(super::fn_metadata::decode_registered_bytes)
            .map(read)
    }
    pub(super) fn write_name(self, bytes: &[u8], infer: bool) -> bool {
        if bytes.is_empty() || (infer && self.with_name(|_| ()).is_some()) {
            return false;
        }
        let state =
            state(self.owner).unwrap_or_else(|| install_state(self.owner, OwnerState::new(None)));
        if let Ok(mut slots) = state.overrides.lock() {
            let index = match slots.binary_search_by_key(&self.ordinal, |s| s.0) {
                Ok(index) => index,
                Err(index) => {
                    slots.insert(index, (self.ordinal, Overrides::default()));
                    index
                }
            };
            let slot = &mut slots[index].1.name;
            if !infer
                || slot
                    .as_deref()
                    .and_then(super::fn_metadata::decode_registered_bytes)
                    .is_none()
            {
                *slot = Some(Arc::from(bytes));
            }
            true
        } else {
            false
        }
    }
    pub(super) fn source(self) -> Option<String> {
        if let Some(state) = state(self.owner) {
            if let Ok(slots) = state.overrides.lock() {
                if let Ok(index) = slots.binary_search_by_key(&self.ordinal, |s| s.0) {
                    if let Some((bytes, _)) = &slots[index].1.source {
                        if let Some(text) = super::fn_metadata::decode_registered_bytes(bytes)
                            .filter(|s| !s.is_empty())
                        {
                            return Some(text.to_owned());
                        }
                    }
                }
            }
        }
        if let Some(copy) = self.copied() {
            return copy
                .source
                .as_deref()
                .and_then(super::fn_metadata::decode_registered_bytes)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
        }
        super::fn_metadata::retained_source_for_info(self.image_info()?)
    }
    pub(super) fn ordinary(self, fallback: bool) -> bool {
        if let Some(state) = state(self.owner) {
            if let Ok(slots) = state.overrides.lock() {
                if let Ok(index) = slots.binary_search_by_key(&self.ordinal, |s| s.0) {
                    if let Some((_, ordinary)) = &slots[index].1.source {
                        return *ordinary;
                    }
                }
            }
        }
        self.copied().map(|e| e.ordinary).unwrap_or_else(|| {
            self.image_info()
                .map(|i| i.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0)
                .unwrap_or(fallback)
        })
    }
    #[cfg(test)]
    pub(super) fn write_source(self, bytes: &[u8], ordinary: bool) {
        let state =
            state(self.owner).unwrap_or_else(|| install_state(self.owner, OwnerState::new(None)));
        let mut slots = state.overrides.lock().unwrap();
        let index = match slots.binary_search_by_key(&self.ordinal, |s| s.0) {
            Ok(index) => index,
            Err(index) => {
                slots.insert(index, (self.ordinal, Overrides::default()));
                index
            }
        };
        slots[index].1.source = Some((Arc::from(bytes), ordinary));
    }
}

/// Borrow the canonical owners for IP resolution. This is a view, not a name
/// snapshot or address-to-id cache. The rare stack path orders actual code
/// locations at query time; ordinary closure reads never search this list.
pub(crate) struct BundleFunctionNames<'a>(&'a [RegisteredOwner]);
impl BundleFunctionNames<'_> {
    fn entries(&self) -> impl Iterator<Item = (usize, &'static [u8])> + '_ {
        self.0.iter().flat_map(|owner| {
            let count = match owner {
                RegisteredOwner::Image(o) => o.count as usize,
                RegisteredOwner::Copied(s) => s.copied.get().map_or(0, Vec::len),
            };
            (0..count).filter_map(move |ordinal| match owner {
                RegisteredOwner::Image(o) => {
                    let info = image_info(*o, ordinal)?;
                    let bytes = super::fn_metadata::info_name_bytes(info)?;
                    (!bytes.is_empty()).then_some((info.code as usize, bytes))
                }
                RegisteredOwner::Copied(s) => {
                    let entry = &(*s).copied.get()?[ordinal];
                    let bytes = entry.name.as_deref()?;
                    (!bytes.is_empty()).then_some((entry.code, bytes))
                }
            })
        })
    }
    pub(crate) fn len(&self) -> usize {
        self.entries().count()
    }
    pub(crate) fn contains_key(&self, address: &usize) -> bool {
        self.entries().any(|(code, _)| code == *address)
    }
    pub(crate) fn predecessor(&self, ip: usize) -> Option<(usize, &'static [u8])> {
        self.entries()
            .filter(|(code, _)| *code <= ip)
            .max_by_key(|(code, _)| *code)
    }
    pub(crate) fn first_key_value(&self) -> Option<(usize, &'static [u8])> {
        self.entries().min_by_key(|(code, _)| *code)
    }
    pub(crate) fn last_key_value(&self) -> Option<(usize, &'static [u8])> {
        self.entries().max_by_key(|(code, _)| *code)
    }
}
pub(crate) fn with_bundle_function_names<R>(
    read: impl FnOnce(&BundleFunctionNames<'_>) -> R,
) -> Option<R> {
    let owners = owners().lock().ok()?;
    Some(read(&BundleFunctionNames(&owners)))
}

/// Raw method/accessor dispatch still carries a code location rather than a
/// closure info. Resolve it against the same canonical descriptors; do not
/// invent an address-keyed registration ABI or a second routing table.
pub(super) fn source_for_code(code: usize) -> Option<String> {
    let owners = owners().lock().ok()?;
    owners.iter().rev().find_map(|owner| match owner {
        RegisteredOwner::Image(owner) => (0..owner.count as usize).rev().find_map(|ordinal| {
            let info = image_info(owner, ordinal)?;
            (info.code as usize == code)
                .then(|| super::fn_metadata::retained_source_for_info(info))
                .flatten()
        }),
        RegisteredOwner::Copied(state) => state
            .copied
            .get()?
            .iter()
            .rev()
            .find(|entry| entry.code == code)
            .and_then(|e| e.source.as_deref())
            .and_then(super::fn_metadata::decode_registered_bytes)
            .map(str::to_owned),
    })
}
pub(super) fn name_for_code(code: usize) -> Option<String> {
    with_bundle_function_names(|names| {
        names
            .entries()
            .filter(|(address, _)| *address == code)
            .last()
            .and_then(|(_, bytes)| super::fn_metadata::decode_registered_bytes(bytes))
            .map(str::to_owned)
    })
    .flatten()
}
pub(crate) fn function_registries_census() -> Vec<crate::gc::census::SideTableRow> {
    let Ok(owners) = owners().lock() else {
        return Vec::new();
    };
    let mut entries = 0;
    let mut bytes = owners.capacity() * std::mem::size_of::<RegisteredOwner>();
    for owner in owners.iter() {
        let state = match owner {
            RegisteredOwner::Image(o) => state(o),
            RegisteredOwner::Copied(s) => Some(*s),
        };
        if let Some(state) = state {
            bytes += std::mem::size_of::<OwnerState>();
            if let Some(copied) = state.copied.get() {
                bytes += copied.capacity() * std::mem::size_of::<CopiedEntry>();
                bytes += copied
                    .iter()
                    .map(|e| {
                        e.name.as_ref().map_or(0, |b| b.len() + 16)
                            + e.source.as_ref().map_or(0, |b| b.len() + 16)
                    })
                    .sum::<usize>();
            }
            if let Ok(slots) = state.overrides.lock() {
                entries += slots.len();
                bytes += slots.capacity() * std::mem::size_of::<(u32, Overrides)>();
                bytes += slots
                    .iter()
                    .map(|(_, v)| {
                        v.name.as_ref().map_or(0, |b| b.len() + 16)
                            + v.source.as_ref().map_or(0, |(b, _)| b.len() + 16)
                    })
                    .sum::<usize>();
            }
        }
    }
    vec![
        ("fn.metadata_owners", owners.len(), bytes),
        ("fn.metadata_overrides(entries)", entries, 0),
    ]
}

#[cfg(test)]
pub(crate) fn test_bundle_names(entries: &[(usize, &[u8])]) -> BundleFunctionNames<'static> {
    let copied = entries
        .iter()
        .map(|(code, bytes)| CopiedEntry {
            code: *code,
            name: Some(Arc::from(*bytes)),
            source: None,
            ordinary: false,
        })
        .collect();
    let state = Box::leak(Box::new(OwnerState::new(Some(copied))));
    BundleFunctionNames(Box::leak(Box::new([RegisteredOwner::Copied(state)])))
}
#[cfg(test)]
pub(crate) fn test_publish_bundle_name(code: usize, bytes: &[u8]) {
    let view = test_bundle_names(&[(code, bytes)]);
    owners().lock().unwrap().extend_from_slice(view.0);
}
#[cfg(test)]
#[path = "fn_metadata_owner_tests.rs"]
mod tests;

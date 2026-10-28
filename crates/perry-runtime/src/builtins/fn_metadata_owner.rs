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
fn state(owner: &FunctionMetadataOwner) -> Option<&'static OwnerState> {
    unsafe {
        owner
            .state
            .load(Ordering::Acquire)
            .cast::<OwnerState>()
            .as_ref()
    }
}
fn install_state(owner: &FunctionMetadataOwner, value: OwnerState) -> &'static OwnerState {
    let pointer = Box::into_raw(Box::new(value));
    match owner.state.compare_exchange(
        std::ptr::null_mut(),
        pointer.cast(),
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => unsafe { &*pointer },
        Err(existing) => unsafe {
            drop(Box::from_raw(pointer));
            &*existing.cast::<OwnerState>()
        },
    }
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
        let already = list.iter().any(|item| match (item, registered) {
            (RegisteredOwner::Image(a), RegisteredOwner::Image(b)) => std::ptr::eq(*a, b),
            (RegisteredOwner::Copied(a), RegisteredOwner::Copied(b)) => std::ptr::eq(*a, b),
            _ => false,
        });
        if !already {
            list.push(registered);
        }
    }
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
    pub(super) fn write_name(self, bytes: &[u8], infer: bool) {
        if bytes.is_empty() || (infer && self.with_name(|_| ()).is_some()) {
            return;
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

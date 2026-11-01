//! Canonical metadata ownership. Startup publishes an owner identity and its
//! image's function records, not function-address keys. Closure operations
//! read the info they hold; only the rare by-address paths (stack frame names,
//! the source of raw method code) walk an image's records.
//!
//! Records (`perry_codegen::fn_record`) live in a linker-owned section, one
//! per body that publishes an info, tied to the body's own input section: the
//! link keeps a record exactly when it keeps the body, so the section between
//! its bounds lists the surviving bodies and nothing roots the others.
use crate::codegen_abi::{
    FunctionMetadataIdentity, FunctionMetadataOwner, FunctionRecordBounds, JsFunctionInfo,
    FUNCTION_RECORD_BYTES,
};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex, OnceLock,
};
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
/// `copied` (an unloadable image's bytes) has one slot per ordinal; the slot
/// of a body the link dropped stays empty.
struct OwnerState {
    copied: OnceLock<Box<[OnceLock<CopiedEntry>]>>,
    /// Set once the owner's copies are listed under their image.
    listed: std::sync::atomic::AtomicBool,
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
            let _ = storage.set(entries.into_iter().map(OnceLock::from).collect());
        }
        Self {
            copied: storage,
            listed: std::sync::atomic::AtomicBool::new(false),
            overrides: Mutex::new(Vec::new()),
            registration: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

/// One image's records, as registration found them: the linker-owned
/// section of a permanent image, or, for an unloadable one, the owners whose
/// copies were taken from it (its section is never read again).
#[derive(Clone, Copy)]
enum ImageRecords {
    Image(&'static FunctionRecordBounds),
    Copied(&'static [&'static OwnerState]),
}
fn images() -> &'static Mutex<Vec<ImageRecords>> {
    static IMAGES: OnceLock<Mutex<Vec<ImageRecords>>> = OnceLock::new();
    crate::once_init::get_or_init(&IMAGES, || Mutex::new(Vec::new()))
}
/// The next owner registration index plus one (CKI S2): owners take indices
/// in the order module init registers them.
static NEXT_REGISTRATION: AtomicU32 = AtomicU32::new(1);

/// The infos of an image's records, in section order. A zero offset is no
/// record (the image sentinel, a section marker, alignment padding).
///
/// # Safety
/// `bounds` names the records section of a loaded image.
unsafe fn image_infos(
    bounds: &FunctionRecordBounds,
) -> impl DoubleEndedIterator<Item = &'static JsFunctionInfo> {
    let start = bounds.start as usize;
    let count = (bounds.stop as usize).saturating_sub(start) / FUNCTION_RECORD_BYTES;
    (0..count).filter_map(move |index| {
        let record = start + index * FUNCTION_RECORD_BYTES;
        // SAFETY: inside the section; records are 4-byte aligned.
        let delta = unsafe { (record as *const i32).read() };
        if delta == 0 {
            return None;
        }
        let info = record.wrapping_add_signed(delta as isize) as *const JsFunctionInfo;
        // SAFETY: a record names the complete info emitted with its body.
        unsafe { info.as_ref() }
    })
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
        let index = if current.is_null() {
            0
        } else {
            (current as usize >> 1) as u32
        };
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
        match owner
            .state
            .compare_exchange(current, tagged, Ordering::AcqRel, Ordering::Acquire)
        {
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

/// Register one module's owner and its image's records (the owner names
/// them). Every ordinal and info already exists; permanent publication does
/// no per-function work.
///
/// # Safety
/// `owner` must be valid for this call. A nonzero `permanent` promises
/// process lifetime; otherwise the image's metadata bytes are copied here,
/// before the image can unload.
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
    // Module init may be repeated by another agent: an owner keeps the index
    // it was first given, and the owner record carries it (CKI S2).
    if registration(owner) == 0 {
        set_registration(owner, NEXT_REGISTRATION.fetch_add(1, Ordering::Relaxed));
    }
    let Some(bounds) = owner.record_bounds() else {
        return;
    };
    let Ok(mut list) = images().lock() else {
        return;
    };
    // Every module of an image passes the same bounds, so the image is added
    // once; images register in runs, so the newest entry is the usual match.
    let known = list.iter().rev().any(|item| match item {
        ImageRecords::Image(b) => std::ptr::eq(*b, bounds),
        ImageRecords::Copied(_) => false,
    });
    if known {
        return;
    }
    if permanent != 0 {
        list.push(ImageRecords::Image(bounds));
    } else {
        list.push(ImageRecords::Copied(copy_image(bounds)));
    }
}
/// An unloadable image: copy every record's bytes into its owner's state
/// now, one slot per ordinal. A cyclic importer may already have written an
/// override; it is preserved. The section is not read again.
///
/// # Safety
/// `bounds` names the records section of a loaded image.
#[cold]
#[inline(never)]
unsafe fn copy_image(bounds: &FunctionRecordBounds) -> &'static [&'static OwnerState] {
    let mut states: Vec<&'static OwnerState> = Vec::new();
    for info in image_infos(bounds) {
        let Some(FunctionMetadataIdentity { owner, ordinal }) = info.metadata_identity() else {
            continue;
        };
        // SAFETY: `metadata_identity` validated the owner pointer.
        let owner = &*owner;
        let owned = state(owner).unwrap_or_else(|| install_state(owner, OwnerState::new(None)));
        let slots = owned
            .copied
            .get_or_init(|| (0..owner.count).map(|_| OnceLock::new()).collect());
        // An ordinal copied twice keeps its first copy, like its identity.
        let _ = slots[ordinal as usize].set(CopiedEntry {
            code: info.code as usize,
            name: super::fn_metadata::info_name_bytes(info).map(Arc::from),
            source: super::fn_metadata::retained_source_for_info(info)
                .map(|s| Arc::from(s.as_bytes())),
            ordinary: info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0,
        });
        if !owned.listed.swap(true, Ordering::AcqRel) {
            states.push(owned);
        }
    }
    Box::leak(states.into_boxed_slice())
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
/// Two words, so it travels in registers; the methods that read the image's
/// bytes take the info the record was made from.
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
    fn copied(self) -> Option<&'static CopiedEntry> {
        state(self.owner)?
            .copied
            .get()?
            .get(self.ordinal as usize)?
            .get()
    }
    pub(super) fn with_name<R>(
        self,
        info: &JsFunctionInfo,
        read: impl FnOnce(&str) -> R,
    ) -> Option<R> {
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
            super::fn_metadata::info_name_bytes(info)
        };
        bytes
            .and_then(super::fn_metadata::decode_registered_bytes)
            .map(read)
    }
    pub(super) fn write_name(self, info: &JsFunctionInfo, bytes: &[u8], infer: bool) -> bool {
        if bytes.is_empty() || (infer && self.with_name(info, |_| ()).is_some()) {
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
    pub(super) fn source(self, info: &JsFunctionInfo) -> Option<String> {
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
        super::fn_metadata::retained_source_for_info(info)
    }
    pub(super) fn ordinary(self, info: &JsFunctionInfo) -> bool {
        if let Some(state) = state(self.owner) {
            if let Ok(slots) = state.overrides.lock() {
                if let Ok(index) = slots.binary_search_by_key(&self.ordinal, |s| s.0) {
                    if let Some((_, ordinary)) = &slots[index].1.source {
                        return *ordinary;
                    }
                }
            }
        }
        self.copied().map_or(
            info.flags & crate::codegen_abi::FN_NON_STRICT_ORDINARY != 0,
            |e| e.ordinary,
        )
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

/// Borrow the images' records for IP resolution. This is a view, not a name
/// snapshot or address-to-id cache. The rare stack path orders actual code
/// locations at query time; ordinary closure reads never search this list.
pub(crate) struct BundleFunctionNames<'a>(&'a [ImageRecords]);
impl BundleFunctionNames<'_> {
    /// Every named body, `(code, name)`, until `visit` returns true. One
    /// non-generic walk shared by every query.
    fn visit(&self, visit: &mut dyn FnMut(usize, &'static [u8]) -> bool) {
        for image in self.0 {
            match *image {
                // SAFETY: a permanent image's records stay mapped.
                ImageRecords::Image(bounds) => {
                    for info in unsafe { image_infos(bounds) } {
                        let Some(bytes) = super::fn_metadata::info_name_bytes(info) else {
                            continue;
                        };
                        if !bytes.is_empty() && visit(info.code as usize, bytes) {
                            return;
                        }
                    }
                }
                ImageRecords::Copied(states) => {
                    for state in states {
                        for entry in state.copied.get().into_iter().flatten() {
                            let Some(entry) = entry.get() else { continue };
                            let Some(bytes) = entry.name.as_deref() else {
                                continue;
                            };
                            if !bytes.is_empty() && visit(entry.code, bytes) {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
    /// The entry the key order picks: the greatest under `better`.
    fn best(
        &self,
        keep: &dyn Fn(usize) -> bool,
        better: &dyn Fn(usize, usize) -> bool,
    ) -> Option<(usize, &'static [u8])> {
        let mut best: Option<(usize, &'static [u8])> = None;
        self.visit(&mut |code, bytes| {
            if keep(code) && best.is_none_or(|(seen, _)| better(code, seen)) {
                best = Some((code, bytes));
            }
            false
        });
        best
    }
    pub(crate) fn len(&self) -> usize {
        let mut count = 0;
        self.visit(&mut |_, _| {
            count += 1;
            false
        });
        count
    }
    pub(crate) fn contains_key(&self, address: &usize) -> bool {
        let mut found = false;
        self.visit(&mut |code, _| {
            found = code == *address;
            found
        });
        found
    }
    /// The named body at the greatest code address at or below `ip` (the
    /// last such entry on a tie, as `max_by_key` picks).
    pub(crate) fn predecessor(&self, ip: usize) -> Option<(usize, &'static [u8])> {
        self.best(&|code| code <= ip, &|code, seen| code >= seen)
    }
    pub(crate) fn first_key_value(&self) -> Option<(usize, &'static [u8])> {
        self.best(&|_| true, &|code, seen| code < seen)
    }
    pub(crate) fn last_key_value(&self) -> Option<(usize, &'static [u8])> {
        self.best(&|_| true, &|code, seen| code >= seen)
    }
}
pub(crate) fn with_bundle_function_names<R>(
    read: impl FnOnce(&BundleFunctionNames<'_>) -> R,
) -> Option<R> {
    let images = images().lock().ok()?;
    Some(read(&BundleFunctionNames(&images)))
}

/// Raw method/accessor dispatch still carries a code location rather than a
/// closure info. Resolve it against the same image records; do not invent an
/// address-keyed registration ABI or a second routing table.
pub(super) fn source_for_code(code: usize) -> Option<String> {
    let images = images().lock().ok()?;
    images.iter().rev().find_map(|image| match *image {
        // SAFETY: a permanent image's records stay mapped.
        ImageRecords::Image(bounds) => unsafe { image_infos(bounds) }
            .rev()
            .find(|info| info.code as usize == code)
            .and_then(super::fn_metadata::retained_source_for_info),
        ImageRecords::Copied(states) => states.iter().rev().find_map(|state| {
            state
                .copied
                .get()?
                .iter()
                .rev()
                .filter_map(OnceLock::get)
                .find(|entry| entry.code == code)
                .and_then(|e| e.source.as_deref())
                .and_then(super::fn_metadata::decode_registered_bytes)
                .map(str::to_owned)
        }),
    })
}
pub(super) fn name_for_code(code: usize) -> Option<String> {
    with_bundle_function_names(|names| {
        let mut last = None;
        names.visit(&mut |address, bytes| {
            if address == code {
                last = Some(bytes);
            }
            false
        });
        last.and_then(super::fn_metadata::decode_registered_bytes)
            .map(str::to_owned)
    })
    .flatten()
}
pub(crate) fn function_registries_census() -> Vec<crate::gc::census::SideTableRow> {
    let Ok(images) = images().lock() else {
        return Vec::new();
    };
    // The owners with a payload, found through the images' records (a census
    // walk, not a list the runtime keeps).
    let mut states: Vec<&'static OwnerState> = Vec::new();
    for image in images.iter() {
        match *image {
            // SAFETY: a permanent image's records stay mapped.
            ImageRecords::Image(bounds) => {
                for info in unsafe { image_infos(bounds) } {
                    if let Some(st) = Record::for_info(info).and_then(|r| state(r.owner)) {
                        if !states.iter().any(|seen| std::ptr::eq(*seen, st)) {
                            states.push(st);
                        }
                    }
                }
            }
            ImageRecords::Copied(copied) => {
                for st in copied {
                    if !states.iter().any(|seen| std::ptr::eq(*seen, *st)) {
                        states.push(st);
                    }
                }
            }
        }
    }
    let mut entries = 0;
    let mut bytes = images.capacity() * std::mem::size_of::<ImageRecords>();
    for state in states {
        bytes += std::mem::size_of::<OwnerState>();
        if let Some(copied) = state.copied.get() {
            bytes += copied.len() * std::mem::size_of::<OnceLock<CopiedEntry>>();
            bytes += copied
                .iter()
                .filter_map(OnceLock::get)
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
    vec![
        ("fn.metadata_images", images.len(), bytes),
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
    let state: &'static OwnerState = Box::leak(Box::new(OwnerState::new(Some(copied))));
    let states: &'static [&'static OwnerState] = Box::leak(Box::new([state]));
    BundleFunctionNames(Box::leak(Box::new([ImageRecords::Copied(states)])))
}
#[cfg(test)]
pub(crate) fn test_publish_bundle_name(code: usize, bytes: &[u8]) {
    let view = test_bundle_names(&[(code, bytes)]);
    images().lock().unwrap().extend_from_slice(view.0);
}
#[cfg(test)]
#[path = "fn_metadata_owner_tests.rs"]
mod tests;

//! Bounded LRU of immutable Perex programs. Identity hits read no source bytes;
//! different string allocations use a content hash followed by exact equality.
//! Both GC pointers are mutable roots. No RegExp header or lastIndex is cached.
use super::flags::CanonicalFlags;
use super::perex_owner::GcProgram;
use super::perex_runtime::EngineError;
use crate::gc::{RuntimeHandle, RuntimeHandleScope, RuntimeRootVisitor};
use crate::string::StringHeader;
use perex::binding::ImmutableProgram;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

const CAPACITY: usize = 512;
const BYTE_LIMIT: usize = 32 * 1024 * 1024;
type ContentKey = (u64, CanonicalFlags);

struct Entry {
    source: *const StringHeader,
    program: *const u8,
    key: ContentKey,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct Cache {
    registered: bool,
    entries: Vec<Option<Entry>>,
    identities: HashMap<(usize, CanonicalFlags), usize>,
    contents: HashMap<ContentKey, Vec<usize>>,
    clock: u64,
    bytes: usize,
}

crate::perry_thread_local! {
    static REGEX_CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}

impl Cache {
    fn hit(&mut self, index: usize) -> *const u8 {
        self.clock += 1;
        let entry = self.entries[index].as_mut().unwrap();
        entry.used = self.clock;
        entry.program
    }

    fn remove(&mut self, index: usize) {
        let entry = self.entries[index].take().unwrap();
        self.identities
            .remove(&(entry.source as usize, entry.key.1));
        let bucket = self.contents.get_mut(&entry.key).unwrap();
        bucket.retain(|&i| i != index);
        if bucket.is_empty() {
            self.contents.remove(&entry.key);
        }
        self.bytes -= entry.bytes;
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_counters(|d| d.cache_evictions += 1);
        }
    }

    fn insert(&mut self, mut entry: Entry) {
        if entry.bytes > BYTE_LIMIT {
            return;
        }
        while self.identities.len() >= CAPACITY || self.bytes + entry.bytes > BYTE_LIMIT {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, e)| e.as_ref().map(|e| (i, e.used)))
                .min_by_key(|&(_, used)| used)
                .unwrap()
                .0;
            self.remove(oldest);
        }
        self.clock += 1;
        entry.used = self.clock;
        let index = self
            .entries
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.entries.len());
        self.identities
            .insert((entry.source as usize, entry.key.1), index);
        self.contents.entry(entry.key).or_default().push(index);
        self.bytes += entry.bytes;
        if index == self.entries.len() {
            self.entries.push(Some(entry));
        } else {
            self.entries[index] = Some(entry);
        }
    }
}

pub(super) fn get_or_compile<'s>(
    scope: &'s RuntimeHandleScope,
    source: &RuntimeHandle<'s>,
    flags: CanonicalFlags,
    compile: impl FnOnce() -> Result<GcProgram<'s>, EngineError>,
) -> Result<GcProgram<'s>, EngineError> {
    // Publication retains OriginalSource even on a content hit where this
    // particular string has never been bound by HeapSubject before.
    source.with_mut_ptr::<StringHeader, _>(|p| crate::string::js_string_addref(p));
    if crate::hot_diag::regex_on() {
        source.with_const_ptr::<StringHeader, _>(|p| {
            crate::hot_diag::regex_with(|d| {
                d.note_new(
                    p as usize,
                    super::string_as_bytes(p),
                    flags.as_str(),
                    false,
                    false,
                )
            });
        });
    }
    let identity = source.with_const_ptr::<StringHeader, _>(|p| (p as usize, flags));
    let hit = REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.registered {
            // Register before the first compilation can poll or publish roots.
            // Programs that never construct a RegExp pay no cache scan cost.
            crate::gc::gc_register_named_mutable_root_scanner(
                "regex.program_cache",
                scan_roots_mut,
            );
            cache.registered = true;
        }
        cache
            .identities
            .get(&identity)
            .copied()
            .map(|index| cache.hit(index))
    });
    if let Some(program) = hit {
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_counters(|d| d.program_identity_hits += 1);
        }
        // No collecting operation between lookup and establishing this root.
        return Ok(unsafe { GcProgram::from_cached(scope, program) });
    }
    let hash = unsafe { source.with_string_bytes(fingerprint) };
    let key = (hash, flags);
    let hit = REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache.contents.get(&key).and_then(|bucket| {
            bucket.iter().copied().find(|&i| {
                let entry = cache.entries[i].as_ref().unwrap();
                unsafe {
                    source.with_string_bytes(|bytes| bytes == super::string_as_bytes(entry.source))
                }
            })
        });
        index.map(|index| cache.hit(index))
    });
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| {
            d.program_hash_bytes +=
                source.with_const_ptr::<StringHeader, _>(|p| unsafe { (*p).byte_len as u64 });
            d.program_content_hits += u64::from(hit.is_some());
        });
    }
    if let Some(program) = hit {
        return Ok(unsafe { GcProgram::from_cached(scope, program) });
    }
    let program = compile()?;
    let bytes = program
        .with_words(|words| words.len() * 4)
        .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?
        + source.with_const_ptr::<StringHeader, _>(|p| unsafe { (*p).byte_len as usize });
    let mut entry = Entry {
        source: std::ptr::null(),
        program: std::ptr::null(),
        key,
        bytes,
        used: 0,
    };
    // GC_STORE_AUDIT(ROOT): both slots become roots in REGEX_CACHE and are
    // visited by scan_roots_mut. Stores/barriers do not collect.
    unsafe {
        source.with_mut_ptr::<StringHeader, _>(|p| {
            crate::string::js_string_addref(p);
            crate::gc::runtime_store_root_raw_mut_ptr_slot(
                &mut entry.source as *mut *const StringHeader as *mut *mut StringHeader,
                p,
            );
        });
        program.with_ptr(|p| {
            crate::gc::runtime_store_root_raw_mut_ptr_slot(
                &mut entry.program as *mut *const u8 as *mut *mut u8,
                p as *mut u8,
            )
        });
    }
    REGEX_CACHE.with(|cache| cache.borrow_mut().insert(entry));
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_with(|d| d.perex_compiles += 1);
    }
    Ok(program)
}

pub(crate) fn scan_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let mut moved = false;
        for entry in cache.entries.iter_mut().flatten() {
            let old = entry.source;
            visitor.visit_tagged_raw_const_ptr_slot(&mut entry.source, crate::value::STRING_TAG);
            visitor.visit_raw_const_ptr_slot(&mut entry.program);
            moved |= old != entry.source;
        }
        if moved {
            let identities = cache
                .entries
                .iter()
                .enumerate()
                .filter_map(|(i, entry)| entry.as_ref().map(|e| ((e.source as usize, e.key.1), i)))
                .collect();
            cache.identities = identities;
        }
    });
}

pub(crate) fn census() -> crate::gc::census::SideTableRow {
    REGEX_CACHE.with(|cache| {
        let cache = cache.borrow();
        let native = cache.entries.capacity() * std::mem::size_of::<Option<Entry>>()
            + cache.identities.capacity() * std::mem::size_of::<((usize, CanonicalFlags), usize)>()
            + cache.contents.capacity() * std::mem::size_of::<(ContentKey, Vec<usize>)>()
            + cache
                .contents
                .values()
                .map(|bucket| bucket.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>();
        ("regex.program_cache", cache.identities.len(), native)
    })
}

#[cfg(test)]
pub(crate) fn clear_for_tests() {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let registered = cache.registered;
        *cache = Cache {
            registered,
            ..Cache::default()
        };
    });
}

#[cfg(test)]
pub(crate) fn reset_registration_for_tests() {
    REGEX_CACHE.with(|cache| cache.borrow_mut().registered = false);
}

#[cfg(test)]
mod tests;

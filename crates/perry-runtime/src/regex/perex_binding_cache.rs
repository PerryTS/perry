//! Validate each immutable GC program once, retaining Perex's owner binding.
//! An operation clones the native owner, so eviction/reentrant compile cannot
//! replace its program. Weak root registrations cover active operations even
//! after eviction; they never keep an otherwise unused native owner alive.
use super::perex_owner::{with_cell_words, GcProgram, OwnerError};
use super::perex_runtime::EngineError;
use crate::gc::RuntimeRootVisitor;
use perex::binding::{BoundProgram, ImmutableProgram};
use perex::Budget;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

const CAPACITY: usize = 512;
const BYTE_LIMIT: usize = 32 * 1024 * 1024;

pub(crate) struct ProgramOwner(Rc<Cell<*const u8>>);
impl ImmutableProgram for ProgramOwner {
    type Error = OwnerError;
    fn with_words<T>(&self, f: impl FnOnce(&[u32]) -> T) -> Result<T, OwnerError> {
        // The registered mutable root is reacquired on every borrow. No view
        // or interior pointer is retained by BoundProgram.
        unsafe { with_cell_words(self.0.get(), f) }
    }
}

pub(crate) struct ValidatedProgram {
    root: Rc<Cell<*const u8>>,
    bound: BoundProgram<ProgramOwner>,
    bytes: usize,
    pub(super) scratch_hint: Cell<(usize, usize)>,
}
impl Deref for ValidatedProgram {
    type Target = BoundProgram<ProgramOwner>;
    fn deref(&self) -> &Self::Target {
        &self.bound
    }
}
pub(crate) type SharedProgram = Rc<ValidatedProgram>;

#[derive(Default)]
struct Bindings {
    registered: bool,
    entries: HashMap<usize, (SharedProgram, u64)>,
    roots: Vec<Weak<Cell<*const u8>>>,
    clock: u64,
    bytes: usize,
}
crate::perry_thread_local! {
    static PROGRAM_BINDINGS: RefCell<Bindings> = RefCell::new(Bindings::default());
}

pub(super) fn bind(
    owner: GcProgram<'_>,
    budget: &mut Budget,
) -> Result<SharedProgram, EngineError> {
    let hit = PROGRAM_BINDINGS.with(|bindings| {
        let mut bindings = bindings.borrow_mut();
        if !bindings.registered {
            crate::gc::gc_register_named_mutable_root_scanner(
                "regex.program_bindings",
                scan_roots_mut,
            );
            bindings.registered = true;
        }
        bindings.clock += 1;
        let clock = bindings.clock;
        owner.with_ptr(|p| {
            bindings
                .entries
                .get_mut(&(p as usize))
                .map(|(program, used)| {
                    *used = clock;
                    Rc::clone(program)
                })
        })
    });
    if let Some(hit) = hit {
        return Ok(hit);
    }
    // Native allocation and validation do not collect. `owner` remains rooted
    // until the weak registration below covers the new immutable owner.
    let root = Rc::new(Cell::new(owner.with_ptr(|p| p)));
    let bound = BoundProgram::new(ProgramOwner(Rc::clone(&root)), budget)
        .map_err(|e| EngineError::Program(e.error))?;
    let bytes = bound
        .with_view(|p| p.size_bytes())
        .map_err(EngineError::Program)?;
    let program = Rc::new(ValidatedProgram {
        root,
        bound,
        bytes,
        scratch_hint: Cell::new((0, 0)),
    });
    // GC_STORE_AUDIT(ROOT): the Cell is registered through PROGRAM_BINDINGS's
    // weak root list. Every live owner, including an evicted operation, is
    // visited by scan_roots_mut. Publishing the root does not collect.
    crate::gc::runtime_write_barrier_root_raw_ptr(program.root.get());
    PROGRAM_BINDINGS.with(|bindings| {
        let mut bindings = bindings.borrow_mut();
        if bindings.roots.len() >= CAPACITY * 2 {
            bindings.roots.retain(|root| root.strong_count() != 0);
        }
        bindings.roots.push(Rc::downgrade(&program.root));
        if bytes <= BYTE_LIMIT {
            while bindings.entries.len() >= CAPACITY || bindings.bytes + bytes > BYTE_LIMIT {
                let oldest = *bindings
                    .entries
                    .iter()
                    .min_by_key(|(_, (_, used))| *used)
                    .unwrap()
                    .0;
                let (removed, _) = bindings.entries.remove(&oldest).unwrap();
                bindings.bytes -= removed.bytes;
            }
            let clock = bindings.clock;
            bindings
                .entries
                .insert(program.root.get() as usize, (Rc::clone(&program), clock));
            bindings.bytes += bytes;
        }
    });
    if crate::hot_diag::regex_on() {
        crate::hot_diag::regex_counters(|d| d.perex_validations += 1);
    }
    Ok(program)
}

pub(crate) fn scan_roots_mut(visitor: &mut RuntimeRootVisitor<'_>) {
    PROGRAM_BINDINGS.with(|bindings| {
        let mut bindings = bindings.borrow_mut();
        let mut moved = false;
        bindings.roots.retain(|weak| {
            let Some(root) = weak.upgrade() else {
                return false;
            };
            let old = root.get();
            let mut current = old;
            visitor.visit_raw_const_ptr_slot(&mut current);
            root.set(current);
            moved |= old != current;
            true
        });
        if moved {
            bindings.entries = bindings
                .entries
                .drain()
                .map(|(_, entry)| (entry.0.root.get() as usize, entry))
                .collect();
        }
    });
}

pub(crate) fn census() -> crate::gc::census::SideTableRow {
    PROGRAM_BINDINGS.with(|bindings| {
        let bindings = bindings.borrow();
        let native = bindings.entries.capacity()
            * std::mem::size_of::<(usize, (SharedProgram, u64))>()
            + bindings.entries.len() * std::mem::size_of::<ValidatedProgram>()
            + bindings.roots.capacity() * std::mem::size_of::<Weak<Cell<*const u8>>>();
        ("regex.program_bindings", bindings.entries.len(), native)
    })
}

#[cfg(test)]
pub(crate) fn clear_for_tests() {
    PROGRAM_BINDINGS.with(|bindings| {
        let mut bindings = bindings.borrow_mut();
        bindings.entries.clear();
        bindings.bytes = 0;
        bindings.roots.retain(|root| root.strong_count() != 0);
    });
}

#[cfg(test)]
pub(super) fn reset_registration_for_tests() {
    PROGRAM_BINDINGS.with(|bindings| bindings.borrow_mut().registered = false);
}

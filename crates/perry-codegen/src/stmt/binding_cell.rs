//! The cell a captured binding lives in, and the birth rule that makes a
//! closure's capture layout its own proof.
//!
//! A binding that a closure captures and someone mutates lives in a GC cell:
//! its scope object (`crate::scope_env`) or its own box. A capturing closure's
//! slot for that binding holds the cell. Every capturing closure is born in
//! `expr/closure.rs`, and [`ensure_capture_cells`] runs there before any slot
//! word is read. When the binding's frame root still holds the TAG_UNDEFINED
//! entry sentinel (its declaring statement did not run on this path: a sibling
//! branch, a skipped hoisted declaration, a switch fallthrough), the cell is
//! minted right there, seeded exactly as its declaring statement seeds it, and
//! published into the root, so the frame and every closure share it.
//!
//! The check costs a compare per cell capture, and its cold arm is an
//! allocation, i.e. a collection point. Most births need neither: a direct
//! statement of an enclosing statement list (a preallocation, a boxed `let`)
//! already stored the cell into the root, and every statement after it in that
//! list, at any depth, runs after that store. [`ReadyCellRoots`] records those
//! roots per statement list, and a birth checks only the others.
//!
//! A capture slot of a cell binding therefore holds a live cell of its
//! declared kind from birth on. Nothing writes the slot afterwards (a write to
//! the binding writes INTO the cell), the collector relocates the cell and
//! rewrites the slot, and a Worker transfer re-creates the cell on the other
//! side. A closure body reads its cells with plain loads and never asks the
//! runtime whether the word is a cell.

use std::collections::HashSet;

use crate::expr::FnCtx;
use crate::native_value::LoweredValue;
use crate::types::{I32, I64};

fn record_control_cell(ctx: &mut FnCtx<'_>, id: u32, note: &'static str, addr: &str) {
    let lowered = LoweredValue::js_value_bits(addr);
    ctx.record_lowered_value(
        "CompilerPrivateAsyncControlCell",
        Some(id),
        note,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        Vec::new(),
    );
}

/// Allocate `id`'s own box, seeded as its declaring statement seeds it: the
/// TDZ sentinel for a lexical binding still in its dead zone (a read throws a
/// ReferenceError), `undefined` otherwise. Compiler-private async control
/// cells are i32/i1 boxes and never TDZ. Returns the raw cell pointer.
pub(crate) fn mint_box_cell(ctx: &mut FnCtx<'_>, id: u32, tdz: bool) -> String {
    if crate::expr::is_compiler_private_async_i32_control_local(ctx, id) {
        let cell = ctx.block().call(I64, "js_i32_box_alloc", &[(I32, "0")]);
        record_control_cell(ctx, id, "primitive_i32_control_cell", &cell);
        return cell;
    }
    if crate::expr::is_compiler_private_async_i1_control_local(ctx, id) {
        let cell = ctx.block().call(I64, "js_bool_box_alloc", &[(I32, "0")]);
        record_control_cell(ctx, id, "primitive_i1_control_cell", &cell);
        return cell;
    }
    let seed = if tdz {
        crate::nanbox::TAG_TDZ_I64
    } else {
        crate::nanbox::TAG_UNDEFINED_I64
    };
    ctx.block().call(I64, "js_box_alloc_bits", &[(I64, seed)])
}

/// Allocate the scope object of the group `members` belong to (`members[0]`'s
/// group), every slot seeded with the TDZ sentinel or `undefined`, and the
/// compiler-private control words with their non-pointer tags. Returns the
/// object's raw user pointer.
pub(crate) fn mint_scope_object(ctx: &mut FnCtx<'_>, members: &[u32], tdz: bool) -> String {
    let slot0 = crate::scope_env::access::slot(ctx, members[0]).expect("scoped");
    let seed = if tdz {
        crate::nanbox::TAG_TDZ_I64
    } else {
        crate::nanbox::TAG_UNDEFINED_I64
    };
    let len = slot0.len.to_string();
    let base = ctx
        .block()
        .call(I64, "js_scope_alloc", &[(I32, &len), (I64, seed)]);
    // Compiler-private control words keep a non-pointer tag in the slot's
    // high half; their typed loads and stores touch only the low bytes.
    for &id in members {
        let (seed, note) = if crate::expr::is_compiler_private_async_i32_control_local(ctx, id) {
            // 0x7FFE_0000_0000_0000 (INT32_TAG)
            ("9222809086901354496", "primitive_i32_control_cell")
        } else if crate::expr::is_compiler_private_async_i1_control_local(ctx, id) {
            // 0x7FFC_0000_0000_0000
            ("9222246136947933184", "primitive_i1_control_cell")
        } else {
            continue;
        };
        let slot = crate::scope_env::access::slot(ctx, id).expect("scoped");
        let addr = crate::scope_env::access::cell_addr(ctx, slot, &base);
        let ptr = ctx.block().inttoptr(I64, &addr);
        ctx.block().store(I64, seed, &ptr);
        record_control_cell(ctx, id, note, &addr);
    }
    base
}

/// Mint the cell of captured binding `id` (a group representative or a boxed
/// binding): the group's scope object, or the binding's own box.
fn mint_binding_cell(ctx: &mut FnCtx<'_>, id: u32) -> String {
    match crate::scope_env::access::slot(ctx, id) {
        Some(slot) => {
            let members = ctx.scope_map.members(slot.rep).to_vec();
            let members = if members.is_empty() {
                vec![id]
            } else {
                members
            };
            mint_scope_object(ctx, &members, slot.tdz)
        }
        None => {
            let tdz = ctx.tdz_boxes.contains(&id);
            mint_box_cell(ctx, id, tdz)
        }
    }
}

/// Cell roots known to hold their cell at the current lowering point.
///
/// One frame per statement list being lowered (`lower_stmts`). A cell store
/// is noted while a statement lowers; when that statement is a direct
/// statement of the list (lowered on its own by the plain statement path) and
/// declares the binding, the root joins the list's frame: every later
/// statement of the list, and every list nested in one, runs after the store.
/// A note any other statement leaves (a nested list's, a fused or versioned
/// group's) is dropped, and a frame is dropped with its list.
#[derive(Default, Debug)]
pub(crate) struct ReadyCellRoots {
    frames: Vec<HashSet<u32>>,
    noted: HashSet<u32>,
}

impl ReadyCellRoots {
    /// A function whose `cells` (its boxed parameters) were stored at entry,
    /// before any statement: they hold their cell everywhere in the body.
    pub(crate) fn with_entry_cells(cells: Vec<u32>) -> Self {
        Self {
            frames: vec![cells.into_iter().collect()],
            noted: HashSet::new(),
        }
    }

    pub(crate) fn push_list(&mut self) {
        self.noted.clear();
        self.frames.push(HashSet::new());
    }

    pub(crate) fn pop_list(&mut self) {
        self.noted.clear();
        self.frames.pop();
    }

    /// A cell was stored into `root` (a binding id or a group representative).
    pub(crate) fn note_stored(&mut self, root: u32) {
        self.noted.insert(root);
    }

    /// Drop the notes of whatever lowered since the last settle.
    pub(crate) fn discard_notes(&mut self) {
        self.noted.clear();
    }

    /// The direct statement that just lowered declares `roots`: the noted
    /// ones among them hold their cell for the rest of the current list.
    pub(crate) fn settle(&mut self, roots: impl IntoIterator<Item = u32>) {
        if let Some(frame) = self.frames.last_mut() {
            for root in roots {
                if self.noted.contains(&root) {
                    frame.insert(root);
                }
            }
        }
        self.noted.clear();
    }

    pub(crate) fn is_ready(&self, root: u32) -> bool {
        self.frames.iter().any(|frame| frame.contains(&root))
    }
}

/// The roots a direct statement declares: a `let`'s binding, a
/// preallocation's bindings and their groups' representatives.
pub(crate) fn declared_cell_roots(ctx: &FnCtx<'_>, stmt: &perry_hir::Stmt) -> Vec<u32> {
    match stmt {
        perry_hir::Stmt::Let { id, .. } => vec![*id],
        perry_hir::Stmt::PreallocateBoxes(ids) | perry_hir::Stmt::PreallocateTdzBoxes(ids) => ids
            .iter()
            .flat_map(|id| [*id, ctx.scope_map.capture_key(*id)])
            .collect(),
        _ => Vec::new(),
    }
}

/// The birth rule (module docs): before a closure reads the words for its
/// `captures`, give every cell binding among them whose frame root still
/// holds the null pointer entry sentinel its cell, published into the root.
/// A binding reached through the running closure's own capture slot needs
/// nothing: that slot was filled by the same rule when the running closure was
/// born.
pub(crate) fn ensure_capture_cells(ctx: &mut FnCtx<'_>, captures: &[u32]) {
    for &id in captures {
        if !ctx.boxed_vars.contains(&id)
            || ctx.module_globals.contains_key(&id)
            || ctx.closure_captures.contains_key(&id)
        {
            continue;
        }
        if ctx.ready_cell_roots.is_ready(id) {
            continue;
        }
        let Some(root) = ctx.locals.get(&id).cloned() else {
            continue;
        };
        let word = ctx.block().load(I64, &root);
        let missing = ctx.block().icmp_eq(I64, &word, "0");
        let mint_idx = ctx.new_block("capture_cell.mint");
        let ready_idx = ctx.new_block("capture_cell.ready");
        let mint_label = ctx.block_label(mint_idx);
        let ready_label = ctx.block_label(ready_idx);
        ctx.block().cond_br(&missing, &mint_label, &ready_label);
        ctx.current_block = mint_idx;
        let cell = mint_binding_cell(ctx, id);
        ctx.block().store(I64, &cell, &root);
        super::record_boxed_slot_js_value_bits(ctx, id, &cell, "capture_cell.minted_at_birth");
        ctx.block().br(&ready_label);
        ctx.current_block = ready_idx;
    }
}

/// True when cell binding `id` has no storage in this context: no capture
/// slot of the running closure, no frame root, no module global. A birth
/// capturing it mints the closure's own cell ([`mint_unrooted_capture_cell`]).
pub(crate) fn capture_has_no_storage(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        && !ctx.locals.contains_key(&id)
        && !ctx.module_globals.contains_key(&id)
}

/// The capture words of one closure birth, in slot order.
///
/// Producing a word never collects, except a mint for a capture with no
/// storage ([`capture_has_no_storage`]): that is an allocation. So a birth
/// mints those cells FIRST, before it reads any other word: no loaded word
/// ever crosses a mint. A minted cell that another mint follows is held in one
/// rooted group until the closure allocation consumes it, and every consumer
/// takes the words through [`BirthWords::read`], below the last collecting
/// step before it; the last mint needs no root. A birth with at most one mint
/// emits no root, and one with none emits exactly what it did.
pub(crate) struct BirthWords {
    words: Vec<String>,
    minted: Vec<(usize, Minted)>,
    group: Option<crate::rooting::RootedGroup<'static>>,
}

enum Minted {
    Raw(String),
    Rooted(crate::rooting::EmittedValue),
}

impl BirthWords {
    /// Mint the cells of `captures` that have no storage here.
    pub(crate) fn new(ctx: &mut FnCtx<'_>, captures: &[u32]) -> Self {
        let slots: Vec<usize> = (0..captures.len())
            .filter(|&i| capture_has_no_storage(ctx, captures[i]))
            .collect();
        let mut group = (slots.len() > 1).then(|| crate::rooting::open_rooted_group(slots.len()));
        let mut minted = Vec::with_capacity(slots.len());
        for (n, &slot) in slots.iter().enumerate() {
            let cell = mint_unrooted_capture_cell(ctx, captures[slot]);
            let held = match group.as_mut() {
                Some(group) if n + 1 < slots.len() => {
                    Minted::Rooted(group.adopt_emitted(ctx, crate::rooting::Repr::Ptr, &cell, true))
                }
                _ => Minted::Raw(cell),
            };
            minted.push((slot, held));
        }
        Self {
            words: Vec::with_capacity(captures.len()),
            minted,
            group,
        }
    }

    /// The next slot's word, just produced.
    pub(crate) fn push(&mut self, word: String) {
        self.words.push(word);
    }

    /// The next slot is a capture with no storage: its cell was minted in
    /// [`BirthWords::new`].
    pub(crate) fn push_minted(&mut self) {
        let slot = self.words.len();
        debug_assert!(self.minted.iter().any(|(s, _)| *s == slot));
        self.words.push(String::new());
    }

    pub(crate) fn len(&self) -> usize {
        self.words.len()
    }

    /// Every word as of HERE: the rooted cells re-read from their slots.
    pub(crate) fn read(&self, ctx: &mut FnCtx<'_>) -> Vec<String> {
        let mut words = self.words.clone();
        for (slot, held) in &self.minted {
            let raw = match held {
                Minted::Raw(cell) => cell.clone(),
                Minted::Rooted(handle) => self
                    .group
                    .as_ref()
                    .expect("a rooted mint has a group")
                    .reread_emitted(ctx, *handle),
            };
            words[*slot] = ctx
                .block()
                .or(I64, &raw, &crate::nanbox::POINTER_TAG.to_string());
        }
        words
    }

    /// Drop the roots, below the last consumer.
    pub(crate) fn release(self, ctx: &mut FnCtx<'_>) {
        if let Some(group) = self.group {
            group.release(ctx);
        }
    }
}

/// A cell binding this context has no storage for (neither a frame root nor
/// a capture slot of its own: an outlined module-entry chunk naming a binding
/// whose root lives in another chunk) still gives the closure a cell of its
/// own, so the slot is a cell like every other. It is seeded `undefined`, the
/// value such a capture always read; it is not the binding's shared storage.
pub(crate) fn mint_unrooted_capture_cell(ctx: &mut FnCtx<'_>, id: u32) -> String {
    match crate::scope_env::access::slot(ctx, id) {
        Some(slot) => {
            let members = ctx.scope_map.members(slot.rep).to_vec();
            let members = if members.is_empty() {
                vec![id]
            } else {
                members
            };
            mint_scope_object(ctx, &members, false)
        }
        None => mint_box_cell(ctx, id, false),
    }
}

/// `PERRY_ASSERT_CAPTURE_CELLS=1`: every closure birth checks each cell word
/// it installs (`js_capture_cell_assert` aborts on a word that is not a live
/// box or scope object). The whole-module form of the birth rule's test: any
/// program compiled this way proves every birth it executes.
pub(crate) fn capture_cell_asserts_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        matches!(
            std::env::var("PERRY_ASSERT_CAPTURE_CELLS").as_deref(),
            Ok("1") | Ok("on") | Ok("true")
        )
    })
}

/// Emit the birth check for one installed cell word (see
/// [`capture_cell_asserts_enabled`]).
pub(crate) fn emit_capture_cell_assert(ctx: &mut FnCtx<'_>, bits: &str) {
    ctx.block()
        .call_void("js_capture_cell_assert", &[(I64, bits)]);
}

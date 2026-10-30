//! Owns function-wide virtual-register numbering and the byte-owner root slots the returning edge of a call must retain. Every `%rN` in a function is minted here, and nothing outside this module reaches into its state.
use super::*;

/// Function-wide register counter shared between all blocks in a function.
///
/// Registers are `%r1`, `%r2`, … unique across the entire function body —
/// LLVM requires SSA value names to be unique per function, not per block.
#[derive(Default)]
pub struct RegCounter {
    value: Cell<u32>,
    /// Exact byte-owner root slots retained on the returning edge of actual
    /// calls. This is compiler state: numeric loads emit no lifetime work.
    byte_owner_root_slots: RefCell<Vec<String>>,
    /// Invoke-EH (#7302): the active EH scopes, innermost last. While
    /// non-empty, every emitted call that can reach `js_throw` becomes an
    /// `invoke` unwinding to the top scope's pad (followed by an inline
    /// continuation label, so the emitting code keeps appending
    /// transparently). Lexical scoping matches the dynamic handler stack:
    /// `lower_try` pushes around the try body only — catch/finally bodies see
    /// the *enclosing* scope, which is exactly where a throw escaping them
    /// lands at runtime. A bracket the function opens inside a handler of its
    /// own pushes an *action* scope (`stmt::eh_actions`) whose pad closes the
    /// bracket and then continues at the enclosing scope's `after_pad`.
    eh_scopes: RefCell<Vec<EhScope>>,
    /// Every alloca this function has published to the precise-root collector
    /// with `js_shadow_slot_bind(idx, ptr)`.
    ///
    /// Recorded at the choke points rather than at the thirteen emit sites,
    /// because a fourteenth site is exactly the thing that gets added without
    /// anyone remembering a register list somewhere else.
    ///
    /// A shadow slot is the one class of alloca whose contents the collector
    /// writes behind generated code's back: an evacuating minor rewrites the
    /// slot to the object's new address. [`crate::root_reload`] reads this set
    /// to find the loads that rewrite makes stale. See
    /// `docs/src/internals/gc-rooting-invariant.md`.
    shadow_slot_allocas: RefCell<HashSet<String>>,
    /// Whether any `js_shadow_slot_bind` / `js_shadow_slot_set` was emitted in
    /// this function, through the same choke points as `shadow_slot_allocas`.
    /// A shadow frame nothing writes holds only zeroed slots, so it roots
    /// nothing; [`crate::function::LlFunction`] renders no push or pop for it.
    shadow_slot_written: Cell<bool>,
    /// #8175: module-level registry of `preserve_nonecc` function symbols
    /// (recursion-participating specialized clones). Injected by
    /// `LlModule::define_function` into every function's counter so the two
    /// call choke points below can stamp the call-site convention without any
    /// per-site threading — a call site whose convention disagrees with its
    /// callee is UB, so the registry, not the emitting code, is the single
    /// source of truth. `None` for functions built outside a module (tests).
    preserve_none_fns: RefCell<Option<Rc<RefCell<HashSet<String>>>>>,
    /// The module's `JsFunctionInfo` requests and facts (`crate::fn_info`),
    /// injected by `LlModule::define_function` so every allocation site can
    /// name its body's info without per-site threading. `None` for
    /// functions built outside a module (tests): the info is then named but
    /// never emitted.
    fn_infos: RefCell<Option<Rc<RefCell<crate::fn_info::FnInfoState>>>>,
    /// `@`-prefixed symbol of the module's null-guard global (the zeroed
    /// `i32` that [`LlBlock::safe_load_i32_from_ptr`] dereferences in place
    /// of a bad handle). Injected by `LlModule::define_function` so the name
    /// can carry the module prefix: codegen-unit splitting promotes the
    /// global to a strong link-visible symbol on ELF/COFF, and an unprefixed
    /// `perry_null_guard_zero` in two split modules is a GNU ld
    /// `multiple definition`. Functions built outside a module (tests) keep
    /// the bare name.
    null_guard_symbol: RefCell<Option<String>>,
    /// Compiler-private validity bits for nested stable-packed loop proofs.
    ///
    /// A guarded inner receiver may keep its raw address across call-free
    /// direct-load/store arms. Every actually executed runtime/indirect call
    /// that can collect or run semantic heap work dirties the active proofs
    /// before control can enter the callee; the next indexed read then reloads
    /// its GC root and revalidates. Root bookkeeping and write barriers are
    /// proof-preserving: they neither collect nor change JS-visible receiver
    /// state. Keeping this at the call-emission choke point makes invalidation
    /// path-sensitive: cold IC misses dirty the proof, while their unexecuted
    /// hot siblings do not impose a revalidation on every read.
    stable_packed_revalidation_slots: RefCell<Vec<String>>,
    /// Dirty flags of the function's hoisted byte-cell access proofs. The same
    /// call-emission choke point sets them; the next byte access revalidates.
    byte_access_dirty_slots: RefCell<Vec<String>>,
    /// Flags of the open class-guard run (`expr::class_guard_memo`). The
    /// same choke point clears them; the run's end unregisters them.
    guard_memo_slots: RefCell<Vec<String>>,
    /// Entry-slot cells of the open Map get-then-set regions
    /// (`expr::map_entry_fusion`). The same choke point zeroes them.
    map_entry_slots: RefCell<Vec<String>>,
    /// The compilation's runtime call-effect table, injected by
    /// `LlModule::define_function` (and the other module entry points) from
    /// `CompileOptions::gc_effects`. `None` for functions built outside a
    /// module (tests): [`Self::gc_effects`] then reads
    /// `GcEffects::outside_compilation`.
    gc_effects: RefCell<Option<Arc<crate::gc_call_effects::GcEffects>>>,
}

impl RegCounter {
    pub fn new() -> Self {
        Self {
            value: Cell::new(0),
            byte_owner_root_slots: RefCell::new(Vec::new()),
            eh_scopes: RefCell::new(Vec::new()),
            shadow_slot_allocas: RefCell::new(HashSet::new()),
            shadow_slot_written: Cell::new(false),
            preserve_none_fns: RefCell::new(None),
            fn_infos: RefCell::new(None),
            null_guard_symbol: RefCell::new(None),
            stable_packed_revalidation_slots: RefCell::new(Vec::new()),
            byte_access_dirty_slots: RefCell::new(Vec::new()),
            guard_memo_slots: RefCell::new(Vec::new()),
            map_entry_slots: RefCell::new(Vec::new()),
            gc_effects: RefCell::new(None),
        }
    }

    /// Install the compilation's call-effect table on this function.
    pub(crate) fn set_gc_effects(&self, effects: Arc<crate::gc_call_effects::GcEffects>) {
        *self.gc_effects.borrow_mut() = Some(effects);
    }

    /// The compilation's call-effect table. A function built outside a module
    /// (unit tests) was given none and reads
    /// [`GcEffects::outside_compilation`](crate::gc_call_effects::GcEffects::outside_compilation).
    pub(crate) fn gc_effects(&self) -> Arc<crate::gc_call_effects::GcEffects> {
        if let Some(effects) = &*self.gc_effects.borrow() {
            return Arc::clone(effects);
        }
        let outside = crate::gc_call_effects::GcEffects::outside_compilation();
        *self.gc_effects.borrow_mut() = Some(Arc::clone(&outside));
        outside
    }

    /// Point this function's null-guard loads at `global` (a bare symbol
    /// name, no `@`). See the `null_guard_symbol` field.
    pub(crate) fn set_null_guard_global(&self, global: &str) {
        *self.null_guard_symbol.borrow_mut() = Some(format!("@{global}"));
    }

    pub(super) fn null_guard_symbol(&self) -> String {
        self.null_guard_symbol
            .borrow()
            .clone()
            .unwrap_or_else(|| format!("@{DEFAULT_NULL_GUARD_GLOBAL}"))
    }

    pub(crate) fn push_stable_packed_revalidation_slot(&self, slot: String) {
        self.stable_packed_revalidation_slots
            .borrow_mut()
            .push(slot);
    }

    pub(crate) fn pop_stable_packed_revalidation_slot(&self, expected: &str) {
        let actual = self
            .stable_packed_revalidation_slots
            .borrow_mut()
            .pop()
            .expect("stable-packed revalidation slot stack underflow");
        debug_assert_eq!(actual, expected);
    }

    pub(super) fn stable_packed_revalidation_slots(&self) -> Vec<String> {
        self.stable_packed_revalidation_slots.borrow().clone()
    }

    /// Function-scoped: a byte access proof lives until the function ends.
    pub(crate) fn push_byte_access_dirty_slot(&self, slot: String) {
        let mut slots = self.byte_access_dirty_slots.borrow_mut();
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }

    pub(super) fn byte_access_dirty_slots(&self) -> Vec<String> {
        self.byte_access_dirty_slots.borrow().clone()
    }

    pub(crate) fn push_guard_memo_slot(&self, slot: String) {
        self.guard_memo_slots.borrow_mut().push(slot);
    }

    pub(crate) fn clear_guard_memo_slots(&self) {
        self.guard_memo_slots.borrow_mut().clear();
    }

    pub(crate) fn push_map_entry_slot(&self, slot: String) {
        self.map_entry_slots.borrow_mut().push(slot);
    }

    pub(crate) fn pop_map_entry_slot(&self, slot: &str) {
        let mut slots = self.map_entry_slots.borrow_mut();
        if let Some(at) = slots.iter().rposition(|s| s == slot) {
            slots.remove(at);
        }
    }

    pub(super) fn map_entry_slots(&self) -> Vec<String> {
        self.map_entry_slots.borrow().clone()
    }

    pub(super) fn guard_memo_slots(&self) -> Vec<String> {
        self.guard_memo_slots.borrow().clone()
    }

    /// Install the module's `preserve_nonecc` symbol registry (#8175). Called
    /// once per function by `LlModule::define_function`; the shared cell means
    /// registration order does not matter — reads happen at call-emission and
    /// render time, both after the specialization plan is final.
    pub(crate) fn set_preserve_none_fns(&self, fns: Rc<RefCell<HashSet<String>>>) {
        *self.preserve_none_fns.borrow_mut() = Some(fns);
    }

    /// Install the module's `JsFunctionInfo` state (`crate::fn_info`).
    pub(crate) fn set_fn_infos(&self, infos: Rc<RefCell<crate::fn_info::FnInfoState>>) {
        *self.fn_infos.borrow_mut() = Some(infos);
    }

    /// The `@`-prefixed info symbol of `body`, recorded as requested.
    pub(crate) fn fn_info_ref(&self, body: &str) -> String {
        match &*self.fn_infos.borrow() {
            Some(infos) => infos.borrow_mut().request(body),
            None => format!("@{}", crate::fn_info::info_symbol(body)),
        }
    }

    /// Record a fact about `body`, a body this module defines.
    pub(crate) fn note_fn_info(
        &self,
        body: &str,
        note: impl FnOnce(&mut crate::fn_info::FnInfoFacts),
    ) {
        if let Some(infos) = &*self.fn_infos.borrow() {
            note(infos.borrow_mut().facts_mut(body));
        }
    }

    /// Whether `callee` must be called with the `preserve_nonecc` convention.
    /// Cheap for the overwhelmingly common miss: only generated clone symbols
    /// contain `$`, so ordinary runtime helpers never reach the set lookup.
    pub(crate) fn callee_preserve_none(&self, callee: &str) -> bool {
        if !callee.contains('$') {
            return false;
        }
        match &*self.preserve_none_fns.borrow() {
            Some(fns) => fns.borrow().contains(callee),
            None => false,
        }
    }

    /// Record a write into this function's shadow frame, and `ptr` when
    /// `callee` is the precise-root bind.
    ///
    /// Called from the two choke points every bind and set form passes through:
    /// [`LlBlock::call_void`] — which carries the direct call AND the slow arm
    /// of #7088's inline diamond, since that arm emits the same call — and
    /// `LlFunction::entry_setup_call_void`, which carries the persistent-slot
    /// bind hoisted into the entry prelude.
    pub(crate) fn note_shadow_slot_write(&self, callee: &str, second_arg: Option<&str>) {
        if callee == "js_shadow_slot_set" {
            self.shadow_slot_written.set(true);
        }
        if callee != "js_shadow_slot_bind" {
            return;
        }
        self.shadow_slot_written.set(true);
        // `js_shadow_slot_bind(i32 idx, ptr %slot)`: the slot is argument two.
        // A non-register operand means the bind was built some other way, and
        // the conservative answer is to record nothing — a slot this set does
        // not name is simply never reloaded.
        if let Some(ptr) = second_arg {
            if ptr.starts_with('%') {
                self.shadow_slot_allocas
                    .borrow_mut()
                    .insert(ptr.to_string());
            }
        }
    }

    /// Whether this function wrote any shadow slot.
    pub(crate) fn shadow_slot_written(&self) -> bool {
        self.shadow_slot_written.get()
    }

    /// The shadow slots bound in this function. See [`crate::root_reload`].
    pub(crate) fn shadow_slot_allocas(&self) -> Ref<'_, HashSet<String>> {
        self.shadow_slot_allocas.borrow()
    }

    /// Enter an invoke-EH handler scope that cannot be chained to: calls
    /// emitted from here until the matching pop unwind to `lpad_label`.
    pub fn push_eh_scope(&self, lpad_label: String) {
        self.push_eh_scope_entry(EhScope::handler(lpad_label, None));
    }

    /// Enter an EH scope: calls emitted from here until the matching pop
    /// unwind to `scope.pad`.
    pub fn push_eh_scope_entry(&self, scope: EhScope) {
        self.eh_scopes.borrow_mut().push(scope);
    }

    pub fn pop_eh_scope(&self) -> Option<EhScope> {
        self.eh_scopes.borrow_mut().pop()
    }

    /// Landing pad of the innermost active handler scope, if any.
    pub fn current_eh_unwind_label(&self) -> Option<String> {
        self.eh_scopes
            .borrow()
            .last()
            .map(|scope| scope.pad.clone())
            .filter(|label| !label.is_empty())
    }

    /// The innermost active EH scope, if any.
    pub fn innermost_eh_scope(&self) -> Option<EhScope> {
        self.eh_scopes.borrow().last().cloned()
    }

    /// Number of active EH scopes (handlers and action scopes).
    pub fn eh_scope_depth(&self) -> usize {
        self.eh_scopes.borrow().len()
    }

    pub fn next(&self) -> u32 {
        let v = self.value.get() + 1;
        self.value.set(v);
        v
    }
}

// The operations over the byte-owner slots live with the state they touch, so
// the block emitter never borrows a field of another module's type.
impl RegCounter {
    /// Record a byte-owner root slot retained across a call edge.
    pub(crate) fn retain_byte_owner_root_slot(&self, slot: &str) {
        let mut slots = self.byte_owner_root_slots.borrow_mut();
        if !slots.iter().any(|existing| existing == slot) {
            slots.push(slot.to_owned());
        }
    }

    /// The byte-owner root slots retained so far, as a snapshot.
    pub(crate) fn byte_owner_root_slots(&self) -> Vec<String> {
        self.byte_owner_root_slots.borrow().clone()
    }
}

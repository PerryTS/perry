//! Frame marks: the managed-stack restore without entry savepoints (ehfree
//! slice 1, design §1.1).
//!
//! # The rule
//!
//! Stacks grow down on every target and no Perry code switches stacks
//! (generators and async functions are state machines; workers have their own
//! thread-local state). So "pushed by a frame deeper than the catching frame
//! G" is exactly "pushed with a mark below G's stack pointer".
//!
//! * Every entry of a managed stack carries a **frame mark**: an address
//!   inside the frame that owns the entry. Runtime (Rust) pushes use
//!   [`frame_mark`], the top of the physical frame it is evaluated in.
//!   Compiled pushes pass their own frame's stack pointer
//!   (`llvm.stacksave`).
//! * The first thing every landing pad runs is
//!   `js_eh_pad_enter(llvm.stacksave())` (the trap shim's pad too). The
//!   unwinder has installed G's stack pointer, so that value sits above every
//!   frame the throw abandoned and at or below everything G and its callers
//!   own. [`unwind_managed_stacks`] pops, on every stack, the entries whose
//!   mark is below it.
//! * Restoring at the pad, not at throw time, is exact: no code runs between
//!   the raise and the first pad, and each pad (catch or cleanup) restores to
//!   its own frame before any of its code can collect or run JS.
//! * For every stack pointer a landing pad can have while an entry is live,
//!   the entries marked below it form the top of the stack, so pop-while is
//!   exact. Marks are non-increasing from the bottom to the top, with one
//!   benign exception: entries an owner pushes itself (marked at the top of
//!   its frame) may sit above entries a helper it called made for it (marked
//!   just below its frame, see [`frame_mark`]). No landing pad lies between
//!   the two marks — it would have to be in the owner itself, and runtime
//!   frames have none — so the two always pop together. `PERRY_EH_CHECK=1`
//!   verifies at every pad that no entry marked below its stack pointer
//!   survived anywhere on any stack.
//! * Entries G itself pushed inside the protected region carry G's own mark
//!   and survive the pad. Compiled code closes them with an action at the pad
//!   (`stmt::eh_actions` in perry-codegen); entries G pushed before the
//!   region must survive, and they do.
//!
//! # Runtime handles: a height, not marks
//!
//! The runtime-handle stack is the one managed stack without marks, and the
//! `new.target` bridges (PR #10564) save the value they displace on it.
//! Handle pushes are the hottest of all (every runtime helper that may
//! allocate roots its locals there), and constructions are next, so a mark
//! per push costs more than every `try` entry it saves (+0.5% instructions on
//! tsc and zod for the handle mark, +49 instructions per runtime construction
//! for a separate bridge stack). Instead a handler scope reads the stack's
//! height when it is entered (`js_eh_handle_height`, one call; the trap shim
//! reads it before its `invoke`), and every pad of the scope passes it to
//! `js_eh_pad_enter`, which truncates the stack to it. Everything above the
//! height was pushed after the scope was entered: by a frame the throw
//! abandoned, or by the catching frame's own `new.target` bracket inside the
//! scope, which the throw left too (a compiled frame owns no other handles).
//! The pad puts `new.target` back to the value the oldest truncated save
//! displaced. The other stacks keep marks: a height per stack per `try` entry
//! would put a dozen thread-local reads back on every `try` (about 15
//! instructions each).
//!
//! # The one obligation of a runtime push
//!
//! The mark must be read in the owning frame or in a helper the owner calls
//! directly ([`frame_mark`] explains why both are exact). A helper deeper
//! than that would mark below the owner's later callees, and a catch inside
//! one of them would pop the entry. Guard constructors and push helpers are
//! therefore `#[inline(always)]`, or take the mark as an argument.
//!
//! WASI keeps setjmp transport (§1.3): `js_try_push` records the trap frame's
//! mark and the longjmp path runs the same [`unwind_managed_stacks`].

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
extern "C" {
    /// The address of the physical frame's return-address slot: its CFA - 8.
    #[cfg(target_arch = "x86_64")]
    #[link_name = "llvm.addressofreturnaddress.p0"]
    fn llvm_addressofreturnaddress() -> *const u8;
    /// The stack pointer at the physical frame's entry: its CFA.
    #[cfg(target_arch = "aarch64")]
    #[link_name = "llvm.sponentry.p0"]
    fn llvm_sponentry() -> *const u8;
}

/// The mark of the frame this is evaluated in: the topmost address of the
/// physical frame (just below its CFA).
///
/// That one address is right for both ways a runtime push happens:
///
/// * In the owning frame X (the push inlined into it): the mark lies in X's
///   frame, above every frame X calls and below every frame that calls X.
/// * In a helper E that X calls directly and that returns while the entry
///   stays pushed (`js_async_context_als_run_enter`, a provider's
///   `try_enter_resource_scope`): the mark lies just below X's frame, so
///   every landing pad in a later callee of X (its stack pointer is at least
///   one return address lower) keeps the entry, and every pad in a caller of
///   X pops it. No pad lives in a runtime or C frame itself, so that is
///   exact.
///
/// A helper two calls below the owner would mark too deep; every helper chain
/// that pushes is therefore `#[inline(always)]` down to this call (module
/// docs). `#[inline(always)]` here is load-bearing for the same reason.
#[inline(always)]
pub fn frame_mark() -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: an LLVM intrinsic with no side effects.
        unsafe { llvm_addressofreturnaddress() as usize }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: an LLVM intrinsic with no side effects.
        unsafe { llvm_sponentry() as usize - 1 }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // WASI and the 32-bit targets: the address of a local the optimizer
        // must keep in memory lies in this frame (on wasm, on the linear
        // stack, the stack the compiled frames' marks live on). A helper that
        // returns while its entry stays pushed marks deeper than the
        // intrinsics above; those targets keep the stricter rule that every
        // push is inlined into its owner.
        let slot = 0u8;
        core::hint::black_box(&slot) as *const u8 as usize
    }
}

/// The mark of the frame that called the physical frame this is evaluated
/// in: its CFA, the caller's stack pointer at the call.
///
/// For an `extern "C"` entry point that compiled code (or any owner) calls
/// directly to open a scope spanning the CALLER's frame — a static method's
/// prologue, a derived constructor's `super()` binding, a compiled
/// temp-root bracket. The entry then belongs to the caller exactly as if the
/// caller had passed `llvm.stacksave()`: the caller's own landing pads keep
/// it (`mark < sp` is false at its own stack pointer) and its callers' pads
/// pop it. Such entry points are `#[inline(never)]`, so the physical frame is
/// always theirs.
#[inline(always)]
pub fn caller_frame_mark() -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: an LLVM intrinsic with no side effects.
        unsafe { llvm_addressofreturnaddress() as usize + 8 }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: an LLVM intrinsic with no side effects.
        unsafe { llvm_sponentry() as usize }
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        frame_mark()
    }
}

/// `PERRY_EH_CHECK=1`: at every pad, check that no entry marked below the
/// pad's stack pointer survived the unwind, on any stack (design §5 test 9).
/// Read once.
pub(crate) fn eh_check_enabled() -> bool {
    per_test_global! {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    }
    *crate::once_init::get_or_init(&ON, || {
        matches!(
            std::env::var("PERRY_EH_CHECK").ok().as_deref(),
            Some("1") | Some("on") | Some("true")
        )
    })
}

/// Check one stack's marks against a pad at `sp` in `PERRY_EH_CHECK` mode:
/// every entry still on the stack must belong to the catching frame or a
/// frame above it.
pub(crate) fn check_marks(marks: impl AsRef<[usize]>, sp: usize, stack: &str) {
    let marks = marks.as_ref();
    if let Some((i, &m)) = marks.iter().enumerate().find(|&(_, &m)| m < sp) {
        eprintln!(
            "perry: FATAL: PERRY_EH_CHECK: {stack} stack entry {i} of {} (mark {m:#x}) belongs \
             to a frame the throw abandoned (pad sp {sp:#x}) and survived the unwind",
            marks.len()
        );
        std::process::abort();
    }
}

/// A managed stack whose entries carry frame marks: one buffer of
/// `(mark, value)` pairs, so a push, pop or truncate touches one vector.
/// Reads and in-place mutation go through the value accessors; every growing
/// operation takes a mark (module docs: the mark names the owning frame).
pub(crate) struct CatchStack<T> {
    entries: Vec<(usize, T)>,
}

impl<T> CatchStack<T> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Push `value`, owned by the frame `mark` names.
    #[inline]
    pub(crate) fn push(&mut self, mark: usize, value: T) {
        self.entries.push((mark, value));
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.entries.pop().map(|(_, value)| value)
    }

    #[inline]
    pub(crate) fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    #[inline]
    pub(crate) fn remove(&mut self, index: usize) -> T {
        self.entries.remove(index).1
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub(crate) fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.entries.iter().any(|(_, v)| v == value)
    }

    #[inline]
    pub(crate) fn last(&self) -> Option<&T> {
        self.entries.last().map(|(_, value)| value)
    }

    #[inline]
    #[allow(dead_code)] // mark-stack accessors kept as one complete API
    pub(crate) fn last_mut(&mut self) -> Option<&mut T> {
        self.entries.last_mut().map(|(_, value)| value)
    }

    #[inline]
    #[allow(dead_code)] // mark-stack accessors kept as one complete API
    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        self.entries.get(index).map(|(_, value)| value)
    }

    #[inline]
    #[allow(dead_code)] // mark-stack accessors kept as one complete API
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.entries.get_mut(index).map(|(_, value)| value)
    }

    /// The values, bottom to top.
    #[inline]
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.entries.iter().map(|(_, value)| value)
    }

    /// The values, bottom to top, for in-place rewriting (GC scanners).
    #[inline]
    pub(crate) fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut T> + ExactSizeIterator {
        self.entries.iter_mut().map(|(_, value)| value)
    }

    /// The length after popping every entry whose mark is below `sp`.
    #[inline]
    pub(crate) fn unwound_len(&self, sp: usize) -> usize {
        self.entries
            .iter()
            .rposition(|&(m, _)| m >= sp)
            .map_or(0, |i| i + 1)
    }

    /// Pop every entry a frame below `sp` pushed (newest first), handing each
    /// to `f`. Returns how many were popped.
    #[allow(dead_code)] // mark-stack accessors kept as one complete API
    pub(crate) fn unwind_to_with(&mut self, sp: usize, mut f: impl FnMut(T)) -> usize {
        let keep = self.unwound_len(sp);
        let popped = self.entries.len() - keep;
        while self.entries.len() > keep {
            if let Some((_, value)) = self.entries.pop() {
                f(value);
            }
        }
        popped
    }

    /// Pop every entry a frame below `sp` pushed.
    #[inline]
    pub(crate) fn unwind_to(&mut self, sp: usize) -> usize {
        let keep = self.unwound_len(sp);
        let popped = self.entries.len() - keep;
        self.entries.truncate(keep);
        popped
    }

    /// Restore a live prefix captured by a wasm lexical handler. Never
    /// resurrect entries already closed by normal control flow.
    #[cfg(any(test, target_os = "wasi"))]
    pub(crate) fn restore_depth(&mut self, depth: usize) {
        debug_assert!(depth <= self.entries.len(), "invalid wasm EH prefix");
        self.entries.truncate(depth);
    }

    /// The marks, bottom to top (`PERRY_EH_CHECK`).
    pub(crate) fn marks(&self) -> Vec<usize> {
        self.entries.iter().map(|&(mark, _)| mark).collect()
    }
}

impl<T> std::ops::Index<usize> for CatchStack<T> {
    type Output = T;
    #[inline]
    fn index(&self, index: usize) -> &T {
        &self.entries[index].1
    }
}

impl<T> std::ops::IndexMut<usize> for CatchStack<T> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.entries[index].1
    }
}

macro_rules! catch_stacks {
    ($($(#[$attr:meta])* $name:ident: unwind: $unwind:path, check: $check:path;)*) => {
        /// Pop, on every managed stack, the entries pushed by frames below
        /// `sp` (module docs), and truncate the runtime-handle stack to
        /// `handle_height` (module docs: handles carry no mark). Runs no JS
        /// and allocates nothing.
        pub(crate) fn unwind_managed_stacks(sp: usize, handle_height: usize) {
            #[cfg(test)]
            if !crate::native_payload::callback_sabotage("unwind_runtime_handles") {
                crate::gc::runtime_handle_stack_restore_to(handle_height);
            }
            #[cfg(not(test))]
            crate::gc::runtime_handle_stack_restore_to(handle_height);
            $(
                $(#[$attr])*
                {
                    #[cfg(test)]
                    if crate::native_payload::callback_sabotage(concat!("unwind_", stringify!($name))) {
                        // Sabotage hook (design §5 test 10): skip this stack.
                    } else {
                        $unwind(sp);
                    }
                    #[cfg(not(test))]
                    $unwind(sp);
                }
            )*
            if eh_check_enabled() {
                $( $(#[$attr])* $check(sp); )*
            }
        }

        #[cfg(all(test, not(windows)))]
        mod wasm_checkpoint_tests {
            use super::tests;
            $($(#[$attr])*
                #[test]
                fn $name() {
                    tests::assert_wasm_depth_restore(stringify!($name), tests::$name::depth, tests::$name::push);
                }
            )*
        }

        #[cfg(test)]
        mod restore_tests {
            use super::tests;
            $($(#[$attr])*
                #[test]
                fn $name() {
                    tests::assert_nested_restore(stringify!($name), tests::$name::depth, tests::$name::push);
                }
            )*
        }
    };
}

catch_stacks! {
    // #788: async-context scopes (`AsyncLocalStorage#run`/`#exit`,
    // `runInAsyncScope`) whose normal restore code the throw skipped. Pure
    // thread-local swaps: no JS runs and nothing allocates. First, so a
    // restored snapshot is in place before any other subsystem is touched.
    async_context: unwind: crate::async_context::unwind_context_guards,
    check: crate::async_context::check_context_guard_marks;
    // #1830, #6951: expression temp roots (compiled FFI-fallback brackets and
    // runtime pushes).
    temp_roots: unwind: crate::gc::temp_roots_unwind_to,
    check: crate::gc::temp_roots_check_marks;
    // #5591: skipped method guards must not wedge the recursion counter.
    call_method: unwind: crate::object::call_method_depth_unwind_to,
    check: crate::object::call_method_depth_check_marks;
    // Includes removal of the process-wide outer-pump contribution.
    pump: unwind: crate::stdlib_pump::pump_depth_unwind_to,
    check: crate::stdlib_pump::pump_depth_check_marks;
    // #9082: abandoned walks must no longer inhibit compaction.
    set_foreach: unwind: crate::set::set_foreach_unwind_to,
    check: crate::set::set_foreach_check_marks;
    map_foreach: unwind: crate::map::map_foreach_unwind_to,
    check: crate::map::map_foreach_check_marks;
    prototype_resolution: unwind: crate::object::prototype_chain::resolution_stack_unwind_to,
    check: crate::object::prototype_chain::resolution_stack_check_marks;
    static_private_owner: unwind: crate::object::static_private_owner_unwind_to,
    check: crate::object::static_private_owner_check_marks;
    private_lexical_brand: unwind: crate::object::private_lexical_brand_unwind_to,
    check: crate::object::private_lexical_brand_check_marks;
    derived_super_binding: unwind: crate::object::derived_super_binding_unwind_to,
    check: crate::object::derived_super_binding_check_marks;
    private_member_access_hints: unwind: crate::object::private_member_access_hints_unwind_to,
    check: crate::object::private_member_access_hints_check_marks;
    // #10848: a throwing native-namespace override must not leave its pair
    // marked running.
    namespace_override: unwind: crate::object::namespace_override_unwind_to,
    check: crate::object::namespace_override_check_marks;
}

#[cfg(test)]
mod tests;

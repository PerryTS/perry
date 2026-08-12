//! Shared harness for the `native_proof_*` suites' artifact-JSON tests
//! (#7493).
//!
//! `PERRY_NATIVE_REPS` / `PERRY_NATIVE_REPS_DIR` are read by `compile_module`
//! from the **process** environment, so they are global to a whole test binary
//! rather than to the test that set them. Both suites had a hand-rolled copy of
//! the same set-compile-restore dance, and both copies had the same two
//! defects. They are fixed once, here, and included by both binaries with
//! `#[path]` so a third copy cannot drift.
//!
//! Defect 1 — **poisoning cascade** (#7490's shape, fixed for
//! `typed_feedback.rs` in #7492). A test that unwinds while holding the lock
//! poisons it, and every later `lock().unwrap()` in the binary then dies with
//! `PoisonError` regardless of its own subject. On `main`,
//! `native_proof_regressions` reported **55 failures at default parallelism and
//! 4 under `--test-threads=1`** — 51 of the 55 were `PoisonError`, and *which*
//! tests they hit was decided by the scheduler. That reads as order-dependent
//! codegen state when it is only lock poisoning.
//!
//! Defect 2 — **env leak on unwind**. The restore was hand-written after the
//! compile, so a panic inside `compile_module` left `PERRY_NATIVE_REPS=1` and a
//! stale `PERRY_NATIVE_REPS_DIR` installed for the rest of the process. Every
//! later compile in the binary — including the many `compile_ir` ones that
//! never take this lock — then wrote artifact JSON into a directory another
//! test was reading, producing the torn `EOF while parsing a value` read that
//! poisoned the mutex in the first place.
//!
//! Each including binary gets its own `ARTIFACT_ENV_LOCK` static and its own
//! copy of the two tests below, which is what we want: the lock is per-process
//! and so is the property being asserted.
//!
//! # Buffer-access proof reading (#7505)
//!
//! The second half of this file is the IR reader both suites use to ask "did
//! this fixture emit an UNCHECKED native buffer element address?". It replaces
//! a module-wide `!ir.contains("getelementptr inbounds i8")`, which was true of
//! the intended instruction and of every unrelated one — the shadow-stack
//! lowering's own inline slot addressing emits that exact text, so sixteen
//! tests reported a stale proof that was never emitted the moment anyone ran
//! the suite under `PERRY_RS4GC=0`. See [`native_buffer_element_geps`].

#![allow(dead_code)]

pub static ARTIFACT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`ARTIFACT_ENV_LOCK`] tolerating a poisoned mutex.
///
/// Recovery is sound because the protected state — the `PERRY_NATIVE_REPS*`
/// env vars — is restored by [`NativeRepsEnv`]'s `Drop` during the same unwind,
/// before the mutex is released. One test's failure must fail that test alone.
pub fn artifact_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ARTIFACT_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// RAII for the `PERRY_NATIVE_REPS*` env vars.
pub struct NativeRepsEnv {
    reps: Option<std::ffi::OsString>,
    dir: Option<std::ffi::OsString>,
    all_typed_clone_rejections: Option<std::ffi::OsString>,
}

impl NativeRepsEnv {
    /// Install the artifact-recording env for the lifetime of the guard.
    /// Hold [`artifact_env_lock`] across this — the vars are process-global.
    pub fn install(dir: &std::path::Path, all_typed_clone_rejections: bool) -> Self {
        let saved = NativeRepsEnv {
            reps: std::env::var_os("PERRY_NATIVE_REPS"),
            dir: std::env::var_os("PERRY_NATIVE_REPS_DIR"),
            all_typed_clone_rejections: std::env::var_os(
                "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS",
            ),
        };
        std::env::set_var("PERRY_NATIVE_REPS", "1");
        std::env::set_var("PERRY_NATIVE_REPS_DIR", dir);
        if all_typed_clone_rejections {
            std::env::set_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS", "1");
        } else {
            std::env::remove_var("PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS");
        }
        saved
    }
}

impl Drop for NativeRepsEnv {
    fn drop(&mut self) {
        fn restore(key: &str, value: Option<&std::ffi::OsString>) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        restore("PERRY_NATIVE_REPS", self.reps.as_ref());
        restore("PERRY_NATIVE_REPS_DIR", self.dir.as_ref());
        restore(
            "PERRY_NATIVE_REPS_ALL_TYPED_CLONE_REJECTIONS",
            self.all_typed_clone_rejections.as_ref(),
        );
    }
}

/// Pick this test's own artifact out of `dir`, tolerating foreign or
/// half-written neighbours.
///
/// `PERRY_NATIVE_REPS_DIR` is process-global while installed, so a test
/// compiling concurrently on another thread — none of which take the lock —
/// also drops its artifact here, possibly still being written when we read.
/// Treat that as noise (it cannot be the subject) instead of unwrapping a torn
/// read into a panic INSIDE the lock, which is what poisons it. If the subject
/// then turns out to be missing, the panic names everything that was skipped,
/// so a genuinely truncated target artifact stays diagnosable.
pub fn artifact_for_module(dir: &std::path::Path, name: &str) -> serde_json::Value {
    let paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let mut skipped = Vec::new();
    for path in paths {
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            skipped.push(format!("{}: unreadable", path.display()));
            continue;
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(err) => {
                skipped.push(format!("{}: {err}", path.display()));
                continue;
            }
        };
        if value["module"] == name {
            return value;
        }
        skipped.push(format!("{}", value["module"]));
    }
    panic!("native reps artifact for {name} not found in {dir:?}; skipped {skipped:?}");
}

/// Sabotage test for [`artifact_env_lock`]: a test that panics while holding
/// the lock must fail *itself* and nothing else.
///
/// This asserts its subject was live rather than merely that nothing threw — it
/// plants the exact #7490 shape, proves the mutex really is poisoned
/// afterwards, and only then demands the accessor still hand out a guard. It
/// fails against a plain `ARTIFACT_ENV_LOCK.lock().unwrap()`, so a green run
/// here is evidence.
///
/// `a` sorts first, so under `--test-threads=1` every other test in the
/// including binary runs under a genuinely poisoned lock and the tolerance is
/// exercised suite-wide rather than in one isolated case.
#[test]
fn artifact_env_lock_is_poison_tolerant_so_one_failure_cannot_cascade() {
    let sabotage = std::panic::catch_unwind(|| {
        let _guard = artifact_env_lock();
        panic!("#7493 sabotage: unwinding while holding ARTIFACT_ENV_LOCK");
    });
    assert!(sabotage.is_err(), "the sabotage panic should have unwound");
    assert!(
        ARTIFACT_ENV_LOCK.is_poisoned(),
        "unwinding out of a lock-holding test should poison ARTIFACT_ENV_LOCK — \
         if it no longer does, this test is no longer exercising its subject"
    );
    let _guard = artifact_env_lock();
}

/// Sabotage test for [`NativeRepsEnv`]: the env must survive an unwind out of
/// the compile it wraps.
#[test]
fn artifact_env_is_restored_even_when_the_compile_unwinds() {
    let _guard = artifact_env_lock();
    let before = std::env::var_os("PERRY_NATIVE_REPS");
    let sabotage = std::panic::catch_unwind(|| {
        let _env = NativeRepsEnv::install(std::path::Path::new("/nonexistent/perry7493"), false);
        assert_eq!(
            std::env::var("PERRY_NATIVE_REPS").ok().as_deref(),
            Some("1"),
            "the guard must actually install the var it claims to restore"
        );
        panic!("#7493 sabotage: unwinding inside the artifact env window");
    });
    assert!(sabotage.is_err(), "the sabotage panic should have unwound");
    assert_eq!(
        std::env::var_os("PERRY_NATIVE_REPS"),
        before,
        "PERRY_NATIVE_REPS leaked out of a panicking compile window"
    );
}

// ---------------------------------------------------------------------------
// #7505: naming the buffer access instead of grepping the module
// ---------------------------------------------------------------------------

/// The `define … { … }` body of the one user function every fixture in this
/// family compiles: `probe`.
///
/// Located from its `define` LINE, never from a substring of a body. #7669 is
/// why that distinction is written down: `fast_clone_slice` cut its slice from
/// the first *substring* match of a block label, which is the `br` terminator
/// four lines above the block it meant to cut — so every negative assertion
/// against that slice had no subject, and had been unable to fail since #7612,
/// on the code that then shipped the #7660 SIGBUS.
pub fn probe_body(ir: &str) -> &str {
    let start = ir
        .match_indices("define ")
        // A `define ` must open a LINE to be a definition; anywhere else it is
        // a mention (a comment, a string, a doc line).
        .filter(|(idx, _)| *idx == 0 || ir.as_bytes()[idx - 1] == b'\n')
        .find(|(idx, _)| {
            let line_end = ir[*idx..].find('\n').map(|o| idx + o).unwrap_or(ir.len());
            let line = &ir[*idx..line_end];
            line.contains("@perry_fn_")
                && line.contains("__probe(")
                && !line.contains("@__perry_wrap_")
        })
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| panic!("no `probe` function in IR:\n{ir}"));
    let end = ir[start..]
        .find("\n}\n")
        .map(|o| start + o + 3)
        .unwrap_or(ir.len());
    &ir[start..end]
}

/// `%reg` -> the text right of `=` on its defining line, within one function.
fn defs(fn_ir: &str) -> std::collections::HashMap<&str, &str> {
    fn_ir
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('%'))
        .filter_map(|line| line.split_once(" = "))
        .map(|(reg, def)| (reg.trim(), def.trim()))
        .collect()
}

/// The base pointer of a `getelementptr … i8, ptr %B, …`.
fn gep_i8_base(def: &str) -> Option<String> {
    let rest = def
        .strip_prefix("getelementptr inbounds i8, ptr ")
        .or_else(|| def.strip_prefix("getelementptr i8, ptr "))?;
    let base = rest.split(',').next()?.trim();
    base.starts_with('%').then(|| base.to_string())
}

/// The slot of a `load ptr, ptr %S`.
fn load_ptr_slot(def: &str) -> Option<String> {
    let rest = def.strip_prefix("load ptr, ptr ")?;
    let slot = rest.split(',').next()?.trim();
    slot.starts_with('%').then(|| slot.to_string())
}

/// The `ptr` operand a `store ptr %V, ptr %S` writes into, with its value.
fn store_ptr_operands(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix("store ptr ")?;
    let (value, slot) = rest.split_once(", ptr ")?;
    let value = value.trim();
    let slot = slot.split(',').next()?.trim();
    (value.starts_with('%') && slot.starts_with('%')).then(|| (value.to_string(), slot.to_string()))
}

/// The allocas holding a Buffer view's DATA pointer in `fn_ir`.
///
/// Two shapes, either of which identifies one, because
/// `emit_buffer_access_pointer` reads the length from a dedicated slot when the
/// view has one and from the header otherwise:
///
///  * the initialising store — the NaN-boxed Buffer value is unboxed
///    (`and i64 …, 0xFFFF_FFFF_FFFF` then `inttoptr`), advanced past its header
///    (`getelementptr i8, ptr %p, i32 <const>`) and stored into the slot; and
///  * the header length read — `load ptr` out of the slot, `getelementptr i8`
///    by the length offset, then `load i32 …, !invariant.load`.
///
/// This is the part that makes the search name its subject. A shadow-stack
/// frame pointer, a string's data pointer and an inline-cache slot are all
/// `ptr` values that get `getelementptr inbounds i8`'d; none of them is ever
/// stored into a slot by *this* pair of shapes.
pub fn buffer_data_slots(fn_ir: &str) -> std::collections::BTreeSet<String> {
    let defs = defs(fn_ir);
    let mut slots = std::collections::BTreeSet::new();

    for line in fn_ir.lines() {
        if let Some((value, slot)) = store_ptr_operands(line) {
            let is_unboxed_data_pointer = defs
                .get(value.as_str())
                .and_then(|def| gep_i8_base(def))
                .and_then(|base| defs.get(base.as_str()).copied())
                .is_some_and(|base_def| base_def.starts_with("inttoptr i64 "));
            if is_unboxed_data_pointer {
                slots.insert(slot);
            }
        }
    }

    for line in fn_ir.lines().map(str::trim) {
        if !line.contains("!invariant.load") {
            continue;
        }
        let Some((_, def)) = line.split_once(" = ") else {
            continue;
        };
        let Some(rest) = def.trim().strip_prefix("load i32, ptr ") else {
            continue;
        };
        let Some(header) = rest.split(',').next().map(str::trim) else {
            continue;
        };
        let slot = defs
            .get(header)
            .and_then(|def| gep_i8_base(def))
            .and_then(|base| defs.get(base.as_str()).copied())
            .and_then(load_ptr_slot);
        if let Some(slot) = slot {
            slots.insert(slot);
        }
    }

    slots
}

/// Every `getelementptr inbounds i8` taken off a pointer loaded out of a
/// buffer data slot — i.e. every UNCHECKED native buffer element address in
/// `fn_ir`, and nothing else.
///
/// The predicate this replaces was `!ir.contains("getelementptr inbounds i8")`
/// over the WHOLE MODULE. It could not distinguish the native buffer GEP it
/// meant to catch from an unrelated proven access elsewhere in the module, nor
/// from any other `inbounds i8` the backend happens to emit — and the
/// shadow-stack lowering's inline slot addressing (#7088) emits exactly that
/// text, which is what actually bit us (#7505).
pub fn native_buffer_element_geps(fn_ir: &str) -> Vec<String> {
    let defs = defs(fn_ir);
    let slots = buffer_data_slots(fn_ir);
    fn_ir
        .lines()
        .map(str::trim)
        .filter_map(|line| line.split_once(" = "))
        .filter(|(_, def)| def.starts_with("getelementptr inbounds i8, ptr "))
        .filter_map(|(dst, def)| {
            let base = gep_i8_base(def)?;
            let slot = defs.get(base.as_str()).copied().and_then(load_ptr_slot)?;
            slots.contains(&slot).then(|| dst.trim().to_string())
        })
        .collect()
}

/// No unchecked native buffer element address was emitted in `fn_ir`.
///
/// **Panics when the function has no buffer view at all**, rather than
/// reporting a clean bill of health: "there is no native GEP" and "there is no
/// buffer here to have one" are the two answers this helper exists to tell
/// apart, and only the first is evidence (CLAUDE.md hazard 4).
pub fn assert_no_native_buffer_element_access(fn_ir: &str, what: &str) {
    let slots = buffer_data_slots(fn_ir);
    assert!(
        !slots.is_empty(),
        "{what}: no Buffer data slot in this function, so `no unchecked native \
         element address` has no subject — the fixture did not lower a buffer \
         view at all:\n{fn_ir}"
    );
    let geps = native_buffer_element_geps(fn_ir);
    assert!(
        geps.is_empty(),
        "{what}: unchecked native buffer element address(es) {geps:?} taken off \
         data slot(s) {slots:?} — the invalidated proof leaked through \
         (#7505):\n{fn_ir}"
    );
}

/// At least one unchecked native buffer element address WAS emitted — the
/// control direction, which is what keeps the negative assertion honest.
pub fn assert_native_buffer_element_access(fn_ir: &str, what: &str) {
    let slots = buffer_data_slots(fn_ir);
    assert!(
        !slots.is_empty(),
        "{what}: no Buffer data slot in this function at all:\n{fn_ir}"
    );
    let geps = native_buffer_element_geps(fn_ir);
    assert!(
        !geps.is_empty(),
        "{what}: expected an unchecked native buffer element address off data \
         slot(s) {slots:?} and found none — if this is right the DETECTOR is \
         broken, and every `assert_no_native_buffer_element_access` in this \
         family is vacuous (#7505):\n{fn_ir}"
    );
}

/// A Buffer element store whose native proof was invalidated must fall back to
/// the checked runtime call, and must not ALSO emit an unchecked native
/// address for the same access.
pub fn assert_buffer_store_uses_dynamic_fallback(ir: &str) {
    let body = probe_body(ir);
    assert!(
        body.contains("call void @js_buffer_set"),
        "stale-proof case should keep the checked Buffer store fallback:\n{body}"
    );
    assert_no_native_buffer_element_access(body, "stale-proof Buffer store");
}

// ---------------------------------------------------------------------------
// Self-tests for the reader above.
//
// Hand-written IR rather than a compile, deliberately: these pin the exact
// discrimination the module-wide grep could not make, and they must keep
// failing for that reason even if the fixtures that produce the real IR move.
// The complementary proof against REAL compiled IR, under both lowerings, is
// `invalidation::the_native_buffer_gep_detector_fires_on_a_proven_store`.
// ---------------------------------------------------------------------------

/// A `probe` body with a Buffer view whose element access WAS proven: the data
/// slot `%d`, and an `inbounds` GEP off the pointer loaded out of it.
#[cfg(test)]
const UNCHECKED_NATIVE_BUFFER_PROBE: &str = "\
define double @perry_fn_m_ts__probe() {
entry.0:
  %d = alloca ptr
  %r10 = and i64 %r9, 281474976710655
  %r11 = inttoptr i64 %r10 to ptr
  %r12 = getelementptr i8, ptr %r11, i32 8
  store ptr %r12, ptr %d
  %r40 = load ptr, ptr %d
  %r44 = getelementptr inbounds i8, ptr %r40, i32 %r39
  store i8 %r45, ptr %r44
  ret double 0.0
}
";

#[test]
fn the_buffer_gep_reader_sees_an_unchecked_native_element_address() {
    let body = probe_body(UNCHECKED_NATIVE_BUFFER_PROBE);
    assert_eq!(
        buffer_data_slots(body).into_iter().collect::<Vec<_>>(),
        vec!["%d".to_string()],
        "the data slot must be found from its initialising store"
    );
    assert_eq!(
        native_buffer_element_geps(body),
        vec!["%r44".to_string()],
        "the inbounds GEP off the data pointer is the unchecked access"
    );
    assert_native_buffer_element_access(body, "self-test control");
}

/// The regression that made the old assertion unable to fail: the shadow-stack
/// lowering's inline slot addressing emits `getelementptr inbounds i8` off a
/// pointer loaded out of a slot that is NOT a buffer view.
///
/// Sabotage record — deleting the `slots.contains(&slot)` filter in
/// [`native_buffer_element_geps`] makes this test report `%r20`, `%r21` and
/// fail, which is the whole point: without that filter the reader degenerates
/// into the module-wide grep it replaced.
#[test]
fn the_buffer_gep_reader_ignores_the_shadow_frames_own_inline_slot_gep() {
    let shadow_frame_probe = "\
define double @perry_fn_m_ts__probe() {
entry.0:
  %r3 = alloca ptr
  %r4 = call ptr @js_shadow_frame_enter(i32 1)
  store ptr %r4, ptr %r3
  %r9 = load ptr, ptr %r3
  %r18 = load ptr, ptr %r9
  %r19 = shl i64 %r14, 4
  %r20 = getelementptr inbounds i8, ptr %r18, i64 %r19
  %r21 = getelementptr inbounds i8, ptr %r20, i64 8
  ret double 0.0
}
";
    let body = probe_body(shadow_frame_probe);
    assert!(
        buffer_data_slots(body).is_empty(),
        "a shadow frame's state slot is not a Buffer data slot"
    );
    assert!(
        native_buffer_element_geps(body).is_empty(),
        "the shadow frame's inline slot GEPs must not read as buffer accesses \
         — this is the exact text that turned sixteen `PERRY_RS4GC=0` tests \
         red for a reason unrelated to their subject (#7505)"
    );
}

/// …and a body with no buffer view at all is REFUSED, not reported clean.
#[test]
fn a_body_with_no_buffer_view_refuses_to_certify_the_absence_of_one() {
    let no_buffer_probe = "\
define double @perry_fn_m_ts__probe() {
entry.0:
  ret double 0.0
}
";
    let refused = std::panic::catch_unwind(|| {
        assert_no_native_buffer_element_access(probe_body(no_buffer_probe), "self-test");
    });
    assert!(
        refused.is_err(),
        "a function with no Buffer view must not satisfy `no unchecked native \
         buffer access` — that is the vacuous pass this reader exists to stop"
    );
}

/// The slice must be the function, not the first line that mentions it.
#[test]
fn probe_body_slices_the_definition_and_not_its_wrapper() {
    let ir = "\
define double @__perry_wrap_perry_fn_m_ts__probe(i64 %this_closure) {
entry.0:
  %w1 = getelementptr inbounds i8, ptr %w0, i32 0
  ret double 0.0
}

define double @perry_fn_m_ts__probe() {
entry.0:
  %r1 = add i32 0, 0
  ret double 0.0
}
";
    let body = probe_body(ir);
    assert!(
        body.starts_with("define double @perry_fn_m_ts__probe()"),
        "the wrapper must not be mistaken for the function under test: {body}"
    );
    assert!(
        !body.contains("%w1"),
        "the slice must stop at the function it opened: {body}"
    );
}

#!/usr/bin/env python3
"""Check native registration fixtures; --run compiles isolated temporary Rust copies.

The default mode checks names, assertions, mutation anchors, and adapter fixture
serialization. It does not claim behavioral results. --run requires an authorized
build host and runs no Cargo. It compiles the real native core and Events registry
source; Events uses an empty native payload fixture, not the full runtime/provider.
It also extracts the actual FFI allocation/removal functions into a native fixture
with a Mutex<HashMap> payload map and an inert runtime-probe registration hook.
This checks their transition calls, not DashMap/provider/runtime integration.
Original source is never modified. Every mutant must compile and fail exactly its
named assertion. Compiler errors and unrelated failures never count as evidence.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCES = {
    "core": ROOT / "crates/perry-ffi/src/native_registration.rs",
    "core_tests": ROOT / "crates/perry-ffi/src/native_registration/tests.rs",
    "events": ROOT / "crates/perry-ext-events/src/registry.rs",
    "ffi": ROOT / "crates/perry-ffi/src/handle.rs",
    "ffi_tests": ROOT / "crates/perry-ffi/src/handle_registration_tests.rs",
}
PREFIX = "native_registration::tests::"
EVENTS_PREFIX = "registry::tests::"
FFI_PREFIX = "handle::registration_tests::"
EXPECTED_COUNTS = {PREFIX: 19, EVENTS_PREFIX: 1, FFI_PREFIX: 4}
EXPECTED_TOTAL = sum(EXPECTED_COUNTS.values())


@dataclass(frozen=True)
class Mutation:
    name: str
    target: str
    anchor: str
    replacement: str
    fixture: str
    message: str


MUTATIONS = [
    Mutation(
        "leased_promotion", "core",
        "if !elapsed || !slot.unleased() {", "if !elapsed {",
        PREFIX + "wrapper_lease_blocks_ordinary_reuse_until_release_and_drain",
        "leased retired id must stay out of freelist",
    ),
    Mutation(
        "domain_collapse", "core",
        "issue_serial(&NEXT_DOMAIN).map(Self)", "issue_serial(&NEXT_DOMAIN).map(|_| Self(1))",
        PREFIX + "equal_numeric_ids_keep_domains_distinct",
        "independent registries must have distinct domains",
    ),
    Mutation(
        "serial_reuse", "core",
        "let identity = NativeRegistrationIdentity {\n            domain,\n            serial,",
        "let identity = NativeRegistrationIdentity {\n            domain,\n            serial: 1,",
        PREFIX + "reused_numeric_id_receives_a_new_serial",
        "re-registration must advance its serial",
    ),
    Mutation(
        "operation_omission", "core",
        "self.wrappers == 0 && self.operations == 0", "self.wrappers == 0",
        PREFIX + "operation_lease_outlives_wrapper_lease",
        "operation lease must block reuse after wrapper release",
    ),
    Mutation(
        "deadline_omission", "core",
        "NativeQuarantine::Until(deadline) => now >= deadline,",
        "NativeQuarantine::Until(_deadline) => true,",
        PREFIX + "deadline_and_lease_are_independent_conditions",
        "zero leases cannot bypass a future deadline",
    ),
    Mutation(
        "serial_wrap", "core", "next.checked_add(1)", "Some(next.wrapping_add(1))",
        PREFIX + "serial_exhaustion_never_wraps",
        "serial exhaustion must reject issuance before wrap",
    ),
    Mutation(
        "explicit_occupied_slot", "core",
        "if slot.phase != Phase::Reusable || !slot.unleased() {", "if false {",
        PREFIX + "explicit_ids_reserve_the_counter_path_and_reject_occupied_slots",
        "explicit insertion must reject an occupied registration",
    ),
    Mutation(
        "explicit_counter_overlap", "core",
        "while state.next_id < self.0.end && state.slots.contains_key(&state.next_id) {",
        "while false {",
        PREFIX + "explicit_ids_reserve_the_counter_path_and_reject_occupied_slots",
        "ordinary allocation must skip an explicit slot",
    ),
    Mutation(
        "explicit_free_row_retained", "core",
        "state.free.retain(|entry| *entry != id);", "// Mutation: retain the consumed free row.",
        PREFIX + "explicit_reuse_consumes_its_freelist_row",
        "explicit reuse must consume its freelist row",
    ),
    Mutation(
        "reserved_duplicate_begin", "core",
        "if slot.phase != Phase::Live || slot.kind != kind {",
        "if !matches!(slot.phase, Phase::Live | Phase::Retiring) || slot.kind != kind {",
        PREFIX + "reserved_slots_retire_once_and_cannot_retire_payload_slots",
        "reserved retirement must begin only once",
    ),
    Mutation(
        "reserved_duplicate_finish", "core",
        "if slot.identity != identity || slot.phase != Phase::Retiring {",
        "if slot.identity != identity || !matches!(slot.phase, Phase::Retiring | Phase::Quarantined(_)) {",
        PREFIX + "reserved_slots_retire_once_and_cannot_retire_payload_slots",
        "completed retirement must not queue twice",
    ),
    Mutation(
        "ordinary_queue_overflow", "core",
        "if queue.len() < self.0.queue_cap {", "if true {",
        PREFIX + "ordinary_overflow_abandons_reuse_without_retaining_queue_ownership",
        "ordinary quarantine capacity must hold",
    ),
    Mutation(
        "deadline_queue_overflow", "core",
        "if queue.len() < self.0.queue_cap {", "if true {",
        PREFIX + "deadline_overflow_and_full_freelist_abandon_reuse",
        "deadline quarantine capacity must hold",
    ),
    Mutation(
        "freelist_overflow", "core",
        "if state.free.len() < self.0.queue_cap {", "if true {",
        PREFIX + "freelist_capacity_counts_preexisting_free_rows",
        "full freelist must reject a later eligible retirement",
    ),
    Mutation(
        "abandoned_explicit_reuse", "core",
        "if slot.phase != Phase::Reusable || !slot.unleased() {",
        "if !matches!(slot.phase, Phase::Reusable | Phase::Abandoned) || !slot.unleased() {",
        PREFIX + "ordinary_overflow_abandons_reuse_without_retaining_queue_ownership",
        "abandoned slot must reject explicit reuse",
    ),
    Mutation(
        "discarded_queue_ownership", "core",
        "} else {\n            Phase::Abandoned\n        };",
        "} else {\n            std::mem::forget(self.clone());\n            Phase::Abandoned\n        };",
        PREFIX + "ordinary_overflow_abandons_reuse_without_retaining_queue_ownership",
        "discarded queue rows must not retain native registry ownership",
    ),
    Mutation(
        "publication_wins_order", "core",
        "if !elapsed || !slot.unleased() {", "if !elapsed {",
        PREFIX + "publication_before_worker_retirement_retains_its_registration",
        "publication ordered before retirement must retain its lease",
    ),
    Mutation(
        "retirement_wins_order", "core",
        "if slot.identity != identity || slot.phase != Phase::Live {\n            return None;\n        }\n        let count = match kind {",
        "if slot.identity != identity {\n            return None;\n        }\n        let count = match kind {",
        PREFIX + "worker_retirement_before_publication_rejects_acquisition",
        "retirement ordered before publication must reject acquisition",
    ),
    Mutation(
        "events_empty_slot_selection", "events",
        "pub(super) fn register_event_emitter_handle(value: EventEmitterHandle) -> Handle {",
        """pub(super) fn register_event_emitter_handle(value: EventEmitterHandle) -> Handle {
    {
        let mut slots = lock_event_emitters();
        if let Some(idx) = slots.iter().position(|slot| slot.is_none()) {
            slots[idx] = Some(Box::new(value));
            return EVENT_EMITTER_HANDLE_ID_START + idx as Handle;
        }
    }""",
        EVENTS_PREFIX + "retained_event_emitter_registration_blocks_empty_slot_reuse",
        "retained Events registration must block empty-slot selection",
    ),

    Mutation(
        "ffi_worker_retirement_omission", "ffi",
        """    let identity = REGISTRATIONS.begin_retirement(handle, NativeRegistrationKind::Payload)?;
    // Retiring blocks acquisition/reuse. Neither payload removal nor its later
    // destructor runs under the registration-state mutex.
    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    assert!(REGISTRATIONS.finish_retirement(identity, quarantine));
    removed""",
        "    HANDLES.remove(&handle).map(|(_, boxed)| boxed)",
        FFI_PREFIX + "worker_retirement_uses_native_state_and_type_mismatch_still_removes",
        "worker retirement must end native availability",
    ),
    Mutation(
        "ffi_retirement_completion_omission", "ffi",
        """    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    assert!(REGISTRATIONS.finish_retirement(identity, quarantine));
    removed""",
        """    let removed = HANDLES.remove(&handle).map(|(_, boxed)| boxed);
    // Mutation: leave the native registration Retiring.
    removed""",
        FFI_PREFIX + "worker_retirement_uses_native_state_and_type_mismatch_still_removes",
        "worker retirement must complete quarantine before reuse",
    ),
    Mutation(
        "ffi_reserved_metadata_omission", "ffi",
        """pub fn reserve_handle_id_in_domain(domain: NativeRegistryDomain) -> Handle {
    let Ok(identity) =
        REGISTRATIONS.begin_registration_in_domain(domain, NativeRegistrationKind::Reserved)
    else {
        return INVALID_HANDLE;
    };
    assert!(REGISTRATIONS.publish(identity));
    identity.numeric_id()
}""",
        """pub fn reserve_handle_id_in_domain(domain: NativeRegistryDomain) -> Handle {
    let _ = domain;
    FFI_HANDLE_ID_END - 1
}""",
        FFI_PREFIX + "duplicate_reserved_free_preserves_one_retirement",
        "reservation must create a native registration",
    ),
]


def fixture_bodies(text: str) -> dict[str, str]:
    """Bound a fixture by the next test item; sufficient for these source files."""
    starts = list(re.finditer(r"#\[test\]\s*fn\s+(\w+)\(\)\s*\{", text))
    return {m.group(1): text[m.end():starts[i + 1].start() if i + 1 < len(starts) else len(text)]
            for i, m in enumerate(starts)}


def source_checks(source: dict[str, str]) -> set[str]:
    fixtures = {PREFIX + name: body for name, body in fixture_bodies(source["core_tests"]).items()}
    fixtures.update({EVENTS_PREFIX + name: body for name, body in fixture_bodies(source["events"]).items()})
    fixtures.update({FFI_PREFIX + name: body for name, body in fixture_bodies(source["ffi_tests"]).items()})
    for prefix, count in EXPECTED_COUNTS.items():
        assert sum(name.startswith(prefix) for name in fixtures) == count, (prefix, count)
    assert len({m.name for m in MUTATIONS}) == len(MUTATIONS)
    for mutation in MUTATIONS:
        text = source[mutation.target]
        assert text.count(mutation.anchor) == 1, (mutation.name, "anchor count", text.count(mutation.anchor))
        assert mutation.anchor != mutation.replacement
        assert mutation.fixture in fixtures, mutation.fixture
        assert mutation.message in fixtures[mutation.fixture], (mutation.name, "assertion must be in the named fixture")

    ffi = (ROOT / "crates/perry-ffi/src/handle.rs").read_text()
    ffi_tests = (ROOT / "crates/perry-ffi/src/handle_registration_tests.rs").read_text()
    # Pure/local fixtures need no global lock. All other original fixtures and
    # every new adapter fixture must hold the same guard for their whole body.
    local = {"const_pointer_root_slot_is_rewritten", "ordinary_quarantine_is_bounded",
             "fresh_id_or_exhausted_flags_the_band_boundary"}
    for name, body in fixture_bodies(ffi).items():
        if name not in local:
            assert "let _serial = RECYCLE_TEST_LOCK" in body, (name, "missing global FFI ordering guard")
    for name, body in fixture_bodies(ffi_tests).items():
        assert "let _serial = super::tests::RECYCLE_TEST_LOCK" in body, (name, "missing sibling FFI ordering guard")
    common = (ROOT / "crates/perry-stdlib/src/common/handle.rs").read_text()
    common_tests = (ROOT / "crates/perry-stdlib/src/common/handle_registration_tests.rs").read_text()
    for name, body in {**fixture_bodies(common), **fixture_bodies(common_tests)}.items():
        assert "let _serial = REGISTRATION_TEST_LOCK" in body, (name, "missing Common ordering guard")
        assert "drain_quarantined_common_handles(" not in body, (name, "shared Common fixture must not drain globally")
    assert "#[cfg(test)]\nuse perry_ffi::NativeQuarantine;" in source["events"]
    assert source["events"].count("NativeQuarantine,") == 0
    return set(fixtures)


def checked_run(argv: list[str], timeout: int = 120) -> str:
    result = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"command exited {result.returncode}: {argv}\n{result.stdout}")
    return result.stdout


def harness_source() -> str:
    # Copy constants from the real Events source. The fixture changes only its
    # payload type, whose fields this registry source never reads.
    events_lib = (ROOT / "crates/perry-ext-events/src/lib.rs").read_text()
    constants = []
    for name in ("EVENT_EMITTER_HANDLE_ID_START", "EVENT_EMITTER_HANDLE_ID_END"):
        match = re.search(rf"const {name}: Handle = (0x[0-9A-Fa-f_]+);", events_lib)
        assert match, name
        constants.append(f"const {name}: Handle = {match.group(1)};")
    return """extern crate self as perry_ffi;
mod native_registration;
pub use native_registration::*;
type Handle = i64;
pub struct EventEmitterHandle;
impl EventEmitterHandle { pub fn new() -> Self { Self } }
pub fn handle_registry_domain() -> NativeRegistryDomain {
    static DOMAIN: std::sync::LazyLock<NativeRegistryDomain> =
        std::sync::LazyLock::new(|| NativeRegistryDomain::new().unwrap());
    *DOMAIN
}
mod registry;
mod handle;
""" + "\n".join(constants) + "\n"



FFI_FUNCTIONS = (
    "handle_registry_domain", "handle_registration", "acquire_handle_registration",
    "drain_quarantined_handles", "register_handle", "reserve_handle_id",
    "reserve_handle_id_in_domain", "free_handle_id", "free_handle_id_until",
    "free_reserved_id", "take_handle", "drop_handle", "drop_handle_until",
    "remove_payload", "handle_exists",
)


def extract_function(text: str, name: str) -> str:
    """Extract balanced braces from these audited native functions, verbatim.

    These functions' comments/strings contain no unpaired literal braces. The
    source preparation gate validates every extracted function before compiling.
    """
    match = re.search(rf"(?m)^(?:pub )?fn {name}\b", text)
    assert match, name
    first = text.index("{", match.end())
    depth = 0
    for end in range(first, len(text)):
        if text[end] == "{":
            depth += 1
        elif text[end] == "}":
            depth -= 1
            if depth == 0:
                return text[match.start():end + 1]
    raise AssertionError((name, "unclosed function"))


def ffi_harness_source(source: str) -> str:
    constants = []
    for name in ("FFI_HANDLE_ID_START", "FFI_HANDLE_ID_END", "FREE_HANDLES_CAP"):
        match = re.search(rf"(?m)^const {name}: ([^=]+)= ([^;]+);", source)
        assert match, name
        constants.append(match.group(0))
    prelude = r'''use crate::{NativeLeaseKind, NativeQuarantine, NativeRegistrationIdentity,
    NativeRegistrationKind, NativeRegistrationLease, NativeRegistrationRegistry, NativeRegistryDomain};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;
type Handle = i64;
const INVALID_HANDLE: Handle = 0;
type Payload = Box<dyn Any + Send + Sync>;
#[derive(Default)]
struct PayloadMap(Mutex<HashMap<Handle, Payload>>);
impl PayloadMap {
    fn insert(&self, id: Handle, value: Payload) -> Option<Payload> {
        self.0.lock().unwrap().insert(id, value)
    }
    fn remove(&self, id: &Handle) -> Option<(Handle, Payload)> {
        self.0.lock().unwrap().remove(id).map(|value| (*id, value))
    }
    fn contains_key(&self, id: &Handle) -> bool { self.0.lock().unwrap().contains_key(id) }
}
static HANDLES: LazyLock<PayloadMap> = LazyLock::new(PayloadMap::default);
static REGISTRATIONS: LazyLock<NativeRegistrationRegistry> = LazyLock::new(|| {
    NativeRegistrationRegistry::new(FFI_HANDLE_ID_START, FFI_HANDLE_ID_END, FREE_HANDLES_CAP)
});
fn ensure_handle_exists_probe_registered() {}
fn with_handle<T: Any + Send + Sync, R, F: FnOnce(&T) -> R>(id: Handle, f: F) -> Option<R> {
    let map = HANDLES.0.lock().unwrap();
    map.get(&id).and_then(|value| value.downcast_ref::<T>().map(f))
}
#[cfg(test)]
mod tests { pub(super) static RECYCLE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(()); }
#[cfg(test)]
#[path = "handle_registration_tests.rs"]
mod registration_tests;
'''
    return prelude + "\n".join(constants) + "\n" + "\n\n".join(extract_function(source, name) for name in FFI_FUNCTIONS) + "\n"


def compile_copy(directory: Path, source: dict[str, str], rustc: str, harness: str) -> Path:
    directory.mkdir()
    (directory / "native_registration.rs").write_text(source["core"])
    (directory / "native_registration").mkdir()
    (directory / "native_registration/tests.rs").write_text(source["core_tests"])
    (directory / "registry.rs").write_text(source["events"])
    (directory / "handle.rs").write_text(ffi_harness_source(source["ffi"]))
    (directory / "handle_registration_tests.rs").write_text(source["ffi_tests"])
    harness_file = directory / "harness.rs"
    harness_file.write_text(harness)
    binary = directory / "native-registration-tests"
    checked_run([rustc, "--edition=2021", "--test", str(harness_file), "-o", str(binary)])
    return binary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="store_true", help="compile/execute native and Events fixtures and all mutants")
    parser.add_argument("--rustc", default="rustc")
    args = parser.parse_args()
    source = {name: path.read_text() for name, path in SOURCES.items()}
    hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in SOURCES.items()}
    fixtures = source_checks(source)
    harness = harness_source()
    extracted_ffi = ffi_harness_source(source["ffi"])
    for mutation in MUTATIONS:
        if mutation.target == "ffi":
            assert mutation.anchor in extracted_ffi, (mutation.name, "mutation must reach extracted adapter code")
    print(f"SOURCE CHECK: {EXPECTED_TOTAL} fixtures (19 core + 1 Events + 4 FFI); {len(MUTATIONS)} mutation cases; global fixture ordering", flush=True)
    for name, digest in hashes.items():
        print(f"SOURCE SHA256 {name}: {digest}", flush=True)
    if not args.run:
        print("No Rust compilation or behavioral/sabotage execution requested.")
        return
    with tempfile.TemporaryDirectory(prefix="native-registration-") as scratch:
        root = Path(scratch)
        clean = compile_copy(root / "clean", source, args.rustc, harness)
        # Also compile without cfg(test): the Events import finding was absent
        # from a test-only build. This verifies unused imports in the real file;
        # it does not replace the full provider's warnings gate.
        checked_run([args.rustc, "--edition=2021", "--crate-type=lib", "-D", "unused-imports",
                     str(root / "clean/harness.rs"), "-o", str(root / "clean/native-registration.rlib")])
        listing = checked_run([str(clean), "--list"])
        actual = set(re.findall(r"^((?:native_registration::tests|registry::tests|handle::registration_tests)::\w+): test$", listing, re.M))
        assert actual == fixtures, listing
        output = checked_run([str(clean), "--test-threads=1"])
        assert f"{EXPECTED_TOTAL} passed; 0 failed" in output, output
        print(output, flush=True)
        for mutation in MUTATIONS:
            changed = dict(source)
            changed[mutation.target] = changed[mutation.target].replace(mutation.anchor, mutation.replacement)
            mutant = compile_copy(root / mutation.name, changed, args.rustc, harness)
            result = subprocess.run([str(mutant), mutation.fixture, "--exact", "--test-threads=1"],
                                    text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=30)
            assert result.returncode != 0, (mutation.name, "mutant unexpectedly passed", result.stdout)
            assert ("0 passed; 1 failed" in result.stdout and mutation.message in result.stdout
                    and f"test {mutation.fixture} ... FAILED" in result.stdout), (mutation.name, "unexpected failure", result.stdout)
            print(f"SABOTAGE {mutation.name}: intended assertion failed in {mutation.fixture}; rc={result.returncode}", flush=True)
            print(result.stdout, flush=True)
        print("CLEAN RESTORE: rerunning the original-source binary", flush=True)
        output = checked_run([str(clean), "--test-threads=1"])
        assert f"{EXPECTED_TOTAL} passed; 0 failed" in output, output
        print(output, flush=True)
    for name, path in SOURCES.items():
        assert hashlib.sha256(path.read_bytes()).hexdigest() == hashes[name], name
    print("Original core, tests, Events registry, and FFI adapter source hashes unchanged.")


if __name__ == "__main__":
    main()

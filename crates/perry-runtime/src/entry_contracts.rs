//! Shared receiver proofs for kind entries. These do not use TS annotations.
//!
//! Tagged-value proofs read the header byte directly (honest pointers:
//! `addr_class::pointer_value_header`); raw/foreign inputs keep admission.

use crate::gc::{GcHeader, GC_FLAG_FORWARDED};
use crate::value::{JSValue, POINTER_MASK};

/// The kind portion of the design's closed precondition vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Arr,
    Str,
    Map,
    Set,
    Closure,
}

impl Kind {
    pub const fn header_type(self) -> u8 {
        match self {
            Self::Arr => crate::gc::GC_TYPE_ARRAY,
            Self::Str => crate::gc::GC_TYPE_STRING,
            Self::Map => crate::gc::GC_TYPE_MAP,
            Self::Set => crate::gc::GC_TYPE_SET,
            Self::Closure => crate::gc::GC_TYPE_CLOSURE,
        }
    }

    /// Proof for an unknown JS value: representation plus a live header kind.
    /// SSO strings are values, but do not satisfy the heap-Str contract.
    #[inline]
    pub fn prove(self, value: f64) -> Option<usize> {
        let value = JSValue::from_bits(value.to_bits());
        if self == Self::Str {
            if !value.is_string() {
                return None;
            }
            return self.prove_raw((value.bits() & POINTER_MASK) as usize);
        }
        // Honest pointers: a POINTER_TAG word names a GC-headed object, so
        // the kind is one header byte; no allocator admission precedes it.
        let header = crate::value::addr_class::pointer_value_header(value.bits())?;
        self.holds_header(header)
            .then_some((value.bits() & POINTER_MASK) as usize)
    }

    /// Legacy raw ABI admission. Do not erase it merely from an annotation.
    #[inline]
    pub fn prove_raw(self, addr: usize) -> Option<usize> {
        // Callers supply candidate raw addresses. Admission runs before
        // the header load: a plausible numerical address is not ownership.
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }?;
        let header = unsafe { header.as_ref() };
        (self.holds_header(header)
            && header.size as usize >= crate::gc::GC_HEADER_SIZE + self.minimum_payload_size())
        .then_some(addr)
    }

    const fn minimum_payload_size(self) -> usize {
        match self {
            Self::Arr => std::mem::size_of::<crate::array::ArrayHeader>(),
            Self::Str => std::mem::size_of::<crate::string::StringHeader>(),
            Self::Map => std::mem::size_of::<crate::map::MapHeader>(),
            Self::Set => std::mem::size_of::<crate::set::SetHeader>(),
            Self::Closure => std::mem::size_of::<crate::closure::ClosureHeader>(),
        }
    }

    #[inline(always)]
    pub fn holds_header(self, header: &GcHeader) -> bool {
        header.obj_type == self.header_type() && header.gc_flags & GC_FLAG_FORWARDED == 0
    }

    /// The assertion uses the same proof as tagged-value admission.
    #[inline(always)]
    pub fn assert_raw(self, addr: usize, operation: &str) {
        #[cfg(perry_check_contracts)]
        if self.prove_raw(addr).is_none() {
            contract_failure(self, addr, operation);
        }
        #[cfg(not(perry_check_contracts))]
        let _ = (self, addr, operation);
    }
}

#[cfg(perry_check_contracts)]
#[cold]
#[optimize(size)]
#[inline(never)]
fn contract_failure(kind: Kind, addr: usize, operation: &str) -> ! {
    contract_violated(operation, &format!("{kind:?}"), "raw-kind", addr as u64, "")
}

/// The one report of a failed contract: the operation, the precondition, the
/// proof source that admitted the value, and its bits. Every contract witness
/// (kind entries, pointer boxing, codegen's typed-element witness) ends here.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) fn contract_violated(
    operation: &str,
    precondition: &str,
    proof: &str,
    bits: u64,
    detail: &str,
) -> ! {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "[perry-contract] op={operation} P={precondition} proof={proof} value=0x{bits:016x}{detail}"
    );
    std::process::abort()
}

/// Contract builds: a `POINTER_TAG` constructor was handed a native id
/// (an id-band payload other than null). Release builds compile the call out.
#[cfg(perry_check_contracts)]
#[inline(always)]
pub(crate) fn assert_heap_payload(addr: usize, operation: &str) {
    if addr != 0 && addr < crate::value::addr_class::HANDLE_BAND_MAX {
        heap_payload_failure(addr, operation);
    }
}

#[cfg(perry_check_contracts)]
#[cold]
#[optimize(size)]
#[inline(never)]
fn heap_payload_failure(addr: usize, operation: &str) -> ! {
    contract_violated(operation, "Heap", "pointer-box", addr as u64, "")
}

/// A runtime asked for checks must never silently run an unchecked archive.
pub(crate) fn require_check_build_if_requested() {
    #[cfg(not(perry_check_contracts))]
    if std::env::var("PERRY_CHECK_CONTRACTS").as_deref() == Ok("1") {
        eprintln!(
            "perry: PERRY_CHECK_CONTRACTS=1 requires a runtime built with PERRY_CHECK_CONTRACTS=1"
        );
        std::process::abort();
    }
}

/// One declaration names the exported symbol (first, as the export inventory
/// expects), the kind and a post-proof body shared with the generic operation. Compiler selection is added during receiver migration;
/// release assertions compile out.
macro_rules! kind_entry {
    ($name:ident, $kind:ident, fn($receiver:ident: $ty:ty $(, $arg:ident: $arg_ty:ty)*) -> $ret:ty $body:block) => {
        #[allow(non_snake_case)]
        #[no_mangle]
        pub extern "C" fn $name($receiver: $ty $(, $arg: $arg_ty)*) -> $ret {
            $crate::entry_contracts::Kind::$kind.assert_raw($receiver as usize, stringify!($name));
            $body
        }
    };
}
pub(crate) use kind_entry;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_proof_rejects_other_kinds_handles_and_primitives() {
        let map = crate::map::js_map_alloc(4);
        let value = crate::value::js_nanbox_pointer(map as i64);
        assert_eq!(Kind::Map.prove(value), Some(map as usize));
        for kind in [Kind::Arr, Kind::Str, Kind::Set, Kind::Closure] {
            assert_eq!(kind.prove(value), None);
        }
        for value in [
            42.0,
            crate::value::js_nanbox_handle(
                crate::value::addr_class::FETCH_HANDLE_BAND_START as i64,
            ),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        ] {
            for kind in [Kind::Arr, Kind::Str, Kind::Map, Kind::Set, Kind::Closure] {
                assert_eq!(kind.prove(value), None);
            }
        }
    }
    #[test]
    fn proven_entries_preserve_length_keys_and_body_identity() {
        unsafe extern "C" {
            fn js_array_length__Arr(ptr: usize) -> u32;
            fn js_string_length__Str(ptr: usize) -> u32;
            fn js_map_size__Map(ptr: usize) -> u32;
            fn js_map_get__Map(ptr: usize, key: f64) -> f64;
            fn js_set_size__Set(ptr: usize) -> u32;
            fn js_set_has__Set(ptr: usize, value: f64) -> i32;
            fn js_closure_info__Closure(ptr: usize) -> usize;
        }
        extern "C" fn body(
            _: *const crate::closure::ClosureHeader,
            _: crate::closure::JsThis,
        ) -> f64 {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
        static INFO: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
            body as crate::codegen_abi::JsBody0<crate::closure::ClosureHeader>,
        );
        let _lock = crate::gc::global_side_table_test_lock();
        let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let array = crate::array::js_array_alloc(4);
        crate::array::js_array_push_f64(array, 42.0);
        let map = crate::map::js_map_alloc(4);
        crate::map::js_map_set(map, -0.0, 7.0);
        let set = crate::set::js_set_alloc(4);
        crate::set::js_set_add(set, -0.0);
        let bytes = "💡x".as_bytes();
        let string = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let closure = crate::closure::js_closure_alloc(&INFO, 0);
        for (kind, addr) in [
            (Kind::Arr, array as usize),
            (Kind::Map, map as usize),
            (Kind::Set, set as usize),
            (Kind::Str, string as usize),
            (Kind::Closure, closure as usize),
        ] {
            assert_eq!(kind.prove_raw(addr), Some(addr), "{kind:?}");
        }
        unsafe {
            assert_eq!(js_array_length__Arr(array as usize), 1);
            assert_eq!(js_string_length__Str(string as usize), 3);
            assert_eq!(js_map_size__Map(map as usize), 1);
            assert_eq!(js_map_get__Map(map as usize, 0.0), 7.0);
            assert_eq!(js_set_size__Set(set as usize), 1);
            assert_eq!(js_set_has__Set(set as usize, 0.0), 1);
            assert_eq!(
                js_closure_info__Closure(closure as usize),
                &INFO as *const _ as usize
            );
        }
    }

    // Exercise the actual exported entry, in a subprocess so SIGABRT cannot
    // terminate the test harness. A successful unchecked return is a failure.
    #[cfg(perry_check_contracts)]
    #[test]
    fn invalid_receivers_abort() {
        unsafe extern "C" {
            fn js_array_length__Arr(ptr: usize) -> u32;
            fn js_string_length__Str(ptr: usize) -> u32;
            fn js_map_size__Map(ptr: usize) -> u32;
            fn js_set_size__Set(ptr: usize) -> u32;
            fn js_closure_info__Closure(ptr: usize) -> usize;
        }
        if let Ok(kind) = std::env::var("PERRY_CONTRACT_TEST_KIND") {
            let wrong = if kind == "Handle" {
                let forged = f64::from_bits(
                    crate::value::POINTER_TAG
                        | crate::value::addr_class::FETCH_HANDLE_BAND_START as u64,
                );
                crate::value::js_nanbox_get_pointer(forged) as usize
            } else if kind == "Extent" {
                let array = crate::array::js_array_alloc(4) as usize;
                let header =
                    unsafe { crate::value::addr_class::try_read_tracked_gc_header(array) }.unwrap();
                // The child aborts before collecting or destroying this deliberately short header.
                unsafe {
                    (*header.as_ptr()).size = crate::gc::GC_HEADER_SIZE as u32;
                }
                array
            } else if kind == "Map" {
                crate::set::js_set_alloc(4) as usize
            } else {
                crate::map::js_map_alloc(4) as usize
            };
            unsafe {
                match kind.as_str() {
                    "Arr" | "Handle" | "Extent" => {
                        js_array_length__Arr(wrong);
                    }
                    "Str" => {
                        js_string_length__Str(wrong);
                    }
                    "Map" => {
                        js_map_size__Map(wrong);
                    }
                    "Set" => {
                        js_set_size__Set(wrong);
                    }
                    "Closure" => {
                        js_closure_info__Closure(wrong);
                    }
                    _ => panic!("unknown fixture kind"),
                }
            }
            panic!("wrong-kind entry returned");
        }
        for kind in ["Arr", "Str", "Map", "Set", "Closure", "Handle", "Extent"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "entry_contracts::tests::invalid_receivers_abort",
                    "--nocapture",
                ])
                .env("PERRY_CONTRACT_TEST_KIND", kind)
                .output()
                .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                assert_eq!(output.status.signal(), Some(6), "{kind}: {:?}", output);
            }
            #[cfg(not(unix))]
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains("[perry-contract]"), "{kind}: {error}");
            let expected = if kind == "Handle" || kind == "Extent" {
                "Arr"
            } else {
                kind
            };
            assert!(error.contains(&format!("P={expected}")), "{kind}: {error}");
        }
    }
}

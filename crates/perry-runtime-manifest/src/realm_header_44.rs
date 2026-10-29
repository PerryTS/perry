//! WRT3a3 proving prototype: `perry_abi::realm_header` derives the recorded
//! header of every entry still held in a companion, from the entry's declared
//! facts, once the baseline corrections below are applied. The build phase
//! moves these facts to their birth sites and deletes this file with the
//! companions; the producer then calls the same derivation.
use crate::realm_json::{parse, Value};
use perry_abi::realm_header::{
    array_reserved, closure_reserved, members_need_descriptors, object_class_id, object_reserved,
    ArrayElements, ClosureBirth, Integrity, MemberAttrs, ObjectBirth, ObjectExotic, ObjectLayout,
    PrototypeMark,
};
use std::collections::BTreeSet;

const COMPANIONS: [&str; 6] = [
    include_str!("../../perry-runtime/src/closure/registry.realm.json"),
    include_str!("../../perry-runtime/src/navigator.realm.json"),
    include_str!("../../perry-runtime/src/object/global_this/array_error.realm.json"),
    include_str!("../../perry-runtime/src/object/global_this/populate.realm.json"),
    include_str!("../../perry-runtime/src/object/native_module.realm.json"),
    include_str!("../../perry-runtime/src/object/native_module/constructor_shapes.realm.json"),
];

/// The human facts of each birth. Element kinds of static arrays, descriptor
/// facts, null prototypes, inheritance and the body's constructor flag are
/// derived from the entry itself, as the producer derives them.
enum Declared {
    /// `Some(kind)`: `value = dynamic` with this declared element kind.
    Array(Option<ArrayElements>, Integrity),
    Object {
        exotic: ObjectExotic,
        ordinary_shape: bool,
        prototype_mark: PrototypeMark,
        integrity: Integrity,
    },
    Closure {
        constructable: bool,
    },
}

use Declared::*;
use Integrity::*;

const fn ordinary(ordinary_shape: bool, integrity: Integrity) -> Declared {
    Object {
        exotic: ObjectExotic::Ordinary,
        ordinary_shape,
        prototype_mark: PrototypeMark::OnFirstLink,
        integrity,
    }
}

const NATIVE: Declared = Object {
    exotic: ObjectExotic::NativeNamespace,
    ordinary_shape: false,
    prototype_mark: PrototypeMark::OnFirstLink,
    integrity: Extensible,
};

const STRINGS: Option<ArrayElements> = Some(ArrayElements::Pointers);

const DECLARED: [(&str, Declared); 44] = [
    // Arrays.
    ("node:process.argv", Array(STRINGS, Extensible)),
    ("process.argv", Array(STRINGS, Extensible)),
    ("node:process.execArgv", Array(STRINGS, Extensible)),
    ("process.execArgv", Array(STRINGS, Extensible)),
    ("navigator.languages", Array(STRINGS, Extensible)),
    ("node:module.globalPaths", Array(STRINGS, Extensible)),
    ("node:tls.rootCertificates", Array(STRINGS, Frozen)),
    ("node:http.METHODS", Array(None, Extensible)),
    ("node:module.builtinModules", Array(None, Frozen)),
    ("node:module.Module.wrapper", Array(None, Extensible)),
    ("node:domain._stack", Array(None, Extensible)),
    ("node:domain.default._stack", Array(None, Extensible)),
    ("Array.prototype", Array(None, Extensible)),
    // Ordinary objects.
    (
        "%Global%",
        Object {
            exotic: ObjectExotic::Ordinary,
            ordinary_shape: false,
            prototype_mark: PrototypeMark::AtBirth,
            integrity: Extensible,
        },
    ),
    (
        "node:events.EventEmitter.prototype",
        ordinary(false, Extensible),
    ),
    (
        "node:events.EventEmitterAsyncResource.prototype",
        ordinary(true, Extensible),
    ),
    ("process.stdin", ordinary(true, Extensible)),
    ("process.stdout", ordinary(true, Extensible)),
    ("process.stderr", ordinary(true, Extensible)),
    ("node:module.constants", ordinary(false, Frozen)),
    (
        "node:module.constants.compileCacheStatus",
        ordinary(false, Frozen),
    ),
    (
        "node:async_hooks.asyncWrapProviders",
        ordinary(false, Frozen),
    ),
    ("node:zlib.codes", ordinary(false, Frozen)),
    ("node:vm.constants", ordinary(false, Frozen)),
    // Native module namespaces.
    ("process", NATIVE),
    ("node:assert.strict", NATIVE),
    ("node:fs.default", NATIVE),
    ("node:http.default", NATIVE),
    ("node:http2.default", NATIVE),
    ("node:https.default", NATIVE),
    ("node:inspector.Network", NATIVE),
    ("node:net.default", NATIVE),
    ("node:path.posix", NATIVE),
    ("node:path.win32", NATIVE),
    ("node:streams", NATIVE),
    ("node:sys.types", NATIVE),
    ("node:vm.default", NATIVE),
    // Closures.
    (
        "node:crypto.KeyObject",
        Closure {
            constructable: true,
        },
    ),
    (
        "node:net.Server",
        Closure {
            constructable: true,
        },
    ),
    (
        "node:net.Socket",
        Closure {
            constructable: true,
        },
    ),
    (
        "node:module.flushCompileCache",
        Closure {
            constructable: false,
        },
    ),
    (
        "node:module.isBuiltin",
        Closure {
            constructable: false,
        },
    ),
    (
        "node:events.EventEmitter.captureRejections.<get>",
        Closure {
            constructable: false,
        },
    ),
    (
        "node:events.EventEmitter.captureRejections.<set>",
        Closure {
            constructable: true,
        },
    ),
];

/// The third baseline correction, header part: (entry, recorded
/// `(reserved, class_id)`, corrected `(reserved, class_id)`, why the recorded
/// word was never a realm fact). The tls.rootCertificates contents correction
/// (120 host certificates -> `value = dynamic`) leaves its header unchanged.
const CORRECTIONS: [(&str, (u16, u32), (u16, u32), &str); 11] = [
    (
        "node:process.execArgv",
        (0x4000, 0),
        (0xa000, 0),
        "dump process: its execArgv was empty",
    ),
    (
        "process.execArgv",
        (0x4000, 0),
        (0xa000, 0),
        "dump process: its execArgv was empty",
    ),
    (
        "node:module.globalPaths",
        (0, 0),
        (0xa000, 0),
        "store order: length published before the string stores",
    ),
    (
        "node:http.METHODS",
        (0, 0),
        (0xa000, 0),
        "store order: length published before the string stores",
    ),
    (
        "node:module.builtinModules",
        (7, 0),
        (0xa007, 0),
        "store order: length published before the string stores",
    ),
    (
        "node:module.Module.wrapper",
        (0, 0),
        (0xa000, 0),
        "store order: length published before the string stores",
    ),
    (
        "node:domain._stack",
        (0x4080, 0),
        (0x4000, 0),
        "allocator guess: js_array_alloc's raw-f64 bit",
    ),
    (
        "node:domain.default._stack",
        (0x4080, 0),
        (0x4000, 0),
        "allocator guess: js_array_alloc's raw-f64 bit",
    ),
    (
        "Array.prototype",
        (0x4080, 0),
        (0x4000, 0),
        "allocator guess: js_array_alloc's raw-f64 bit",
    ),
    (
        "process.stdin",
        (0x200, 0),
        (0xa00, 0),
        "birth path: hidden state keys bypass the descriptor funnel",
    ),
    (
        "node:vm.constants",
        (0x807, 0xffff_fffe),
        (0x847, 0),
        "birth order: native namespace retargeted to null after birth",
    ),
];

fn entries() -> Vec<Value> {
    let mut out = Vec::new();
    for source in COMPANIONS {
        let doc = parse(source).unwrap();
        let Some(Value::Object(groups)) = doc.get("groups") else {
            panic!("companion without groups")
        };
        for group in groups.values() {
            out.extend(group.array().unwrap().iter().cloned());
        }
    }
    out
}

fn number(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.parse().unwrap(),
        other => panic!("not a number: {other:?}"),
    }
}

fn properties(entry: &Value) -> &[Value] {
    entry
        .get("properties")
        .and_then(Value::array)
        .unwrap_or(&[])
}

/// What a static array's dense elements hold, from its declared elements.
fn static_elements(entry: &Value) -> ArrayElements {
    let (mut pointers, mut scalars) = (false, false);
    for p in properties(entry) {
        let key = p.get("key").and_then(Value::string).unwrap();
        if p.get("symbol") == Some(&Value::Bool(true)) || key.parse::<u32>().is_err() {
            continue;
        }
        match p
            .get("value")
            .and_then(|v| v.get("kind"))
            .and_then(Value::string)
        {
            Some("string" | "reference" | "bytes" | "symbol") => pointers = true,
            _ => scalars = true,
        }
    }
    match (pointers, scalars) {
        (true, false) => ArrayElements::Pointers,
        (true, true) => ArrayElements::Mixed,
        _ => ArrayElements::Scalars,
    }
}

fn members(entry: &Value) -> Vec<MemberAttrs> {
    properties(entry)
        .iter()
        .map(|p| MemberAttrs {
            symbol: p.get("symbol") == Some(&Value::Bool(true)),
            accessor: p.get("get").is_some() || p.get("set").is_some(),
            attributes: number(p.get("attributes")) as u8,
        })
        .collect()
}

/// (gc_type, reserved, class_id) derived from the declared facts.
fn derive(entry: &Value, declared: &Declared, inherited: &BTreeSet<String>) -> (u64, u16, u32) {
    match declared {
        Array(dynamic, integrity) => {
            let elements = dynamic.unwrap_or_else(|| static_elements(entry));
            (1, array_reserved(elements, *integrity), 0)
        }
        Object {
            exotic,
            ordinary_shape,
            prototype_mark,
            integrity,
        } => {
            let name = entry.get("name").and_then(Value::string).unwrap();
            let prototype_mark = if inherited.contains(name) {
                PrototypeMark::AtBirth
            } else {
                *prototype_mark
            };
            let birth = ObjectBirth {
                exotic: *exotic,
                null_prototype: entry.get("prototype").is_none(),
                layout: if *ordinary_shape {
                    ObjectLayout::PlainOrdinary
                } else {
                    ObjectLayout::Unmarked
                },
                prototype_mark,
                descriptors: members_need_descriptors(&members(entry)),
                typed_array_prototype: false,
                integrity: *integrity,
            };
            (2, object_reserved(birth), object_class_id(*exotic))
        }
        Closure { constructable } => {
            let flags = number(entry.get("function_info").and_then(|i| i.get("flags")));
            let birth = ClosureBirth {
                pointer_free_captures: false,
                constructable: *constructable,
                body_non_constructor: flags & u64::from(perry_abi::FN_NON_CONSTRUCTOR) != 0,
                integrity: Extensible,
            };
            (4, closure_reserved(birth), 0)
        }
    }
}

#[test]
fn every_companion_header_follows_from_its_declared_birth() {
    let entries = entries();
    assert_eq!(
        entries.len(),
        DECLARED.len(),
        "the companion worklist changed"
    );
    let inherited: BTreeSet<String> = entries
        .iter()
        .filter_map(|e| e.get("prototype").and_then(Value::string))
        .map(str::to_owned)
        .collect();
    let mut failures = Vec::new();
    for (name, declared) in &DECLARED {
        let entry = entries
            .iter()
            .find(|e| e.get("name").and_then(Value::string) == Some(name))
            .unwrap_or_else(|| panic!("{name} is not in a companion"));
        let header = entry.get("header").unwrap();
        let recorded = (
            number(header.get("gc_type")),
            number(header.get("reserved")) as u16,
            header.get("class_id").map_or(0, |c| number(Some(c))) as u32,
        );
        let expected = match CORRECTIONS.iter().find(|c| c.0 == *name) {
            Some((_, before, after, _)) => {
                assert_eq!(
                    (recorded.1, recorded.2),
                    *before,
                    "{name}: correction no longer matches the reference"
                );
                (recorded.0, after.0, after.1)
            }
            None => recorded,
        };
        let derived = derive(entry, declared, &inherited);
        if derived != expected {
            failures.push(format!(
                "{name}: derived {derived:?}, recorded {recorded:?}, expected {expected:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_correction_changes_the_recorded_word() {
    for (name, before, after, why) in CORRECTIONS {
        assert_ne!(before, after, "{name}: {why}");
        assert!(DECLARED.iter().any(|(n, _)| *n == name), "{name}");
    }
}

/// The derivation's bit names must be the runtime's until the runtime
/// re-exports them from `perry-abi` (build phase).
#[test]
fn derivation_bits_are_the_runtime_bits() {
    let gc = super::info::gc_constants().unwrap();
    let layout =
        super::info::constants(include_str!("../../perry-runtime/src/gc/layout.rs")).unwrap();
    let native = super::info::constants(include_str!(
        "../../perry-runtime/src/object/native_module.rs"
    ))
    .unwrap();
    for (name, value) in [
        ("OBJ_FLAG_FROZEN", perry_abi::OBJ_FLAG_FROZEN),
        ("OBJ_FLAG_SEALED", perry_abi::OBJ_FLAG_SEALED),
        ("OBJ_FLAG_NO_EXTEND", perry_abi::OBJ_FLAG_NO_EXTEND),
        ("OBJ_FLAG_NULL_PROTO", perry_abi::OBJ_FLAG_NULL_PROTO),
        ("OBJ_FLAG_NEVER_MOVE", perry_abi::OBJ_FLAG_NEVER_MOVE),
        (
            "OBJ_FLAG_HAS_DESCRIPTORS",
            perry_abi::OBJ_FLAG_HAS_DESCRIPTORS,
        ),
        (
            "OBJ_FLAG_TYPED_ARRAY_PROTO",
            perry_abi::realm_header::OBJ_FLAG_TYPED_ARRAY_PROTO,
        ),
        (
            "GC_CLOSURE_NON_CONSTRUCTABLE",
            perry_abi::realm_header::GC_CLOSURE_NON_CONSTRUCTABLE,
        ),
    ] {
        assert_eq!(gc.get(name).copied(), Some(u64::from(value)), "{name}");
    }
    assert_eq!(
        layout.get("GC_LAYOUT_STATE_MASK").copied(),
        Some(u64::from(
            perry_abi::GC_LAYOUT_POINTER_FREE | perry_abi::GC_LAYOUT_SIDE_MASK
        ))
    );
    assert_eq!(
        native.get("NATIVE_MODULE_CLASS_ID").copied(),
        Some(u64::from(perry_abi::realm_header::NATIVE_MODULE_CLASS_ID))
    );
}

//! PEELK18 (ENUM on JSON.parse values; rule R, the for-in subject path re-read): skinbench GLTFLoader `addUnknownExtensionsToUserData`
//! (`for (const name in objectDef.extensions) ... gltfExtensions[name] =
//! objectDef.extensions[name]`): does an Array.prototype method reach a
//! json object's own field?
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

/// `body` runs with `j` (parsed), `obj` (a program object whose userData
/// got json extras), `unknown`, then `j.nodes[0].foo` is called with this
/// = `kept`: an Array.prototype method there (map, forEach, ...) would
/// hand `kept` to `unknown`.
fn src(extras: bool, body: &str) -> String {
    let ex = if extras { "Object.assign(obj.userData, j.extras);" } else { "" };
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         const j: any = JSON.parse(process.argv[3]);
         const obj: any = {{ userData: {{}} }};
         {ex}
         {body}
         const f: any = j.nodes[0].foo;
         try {{ f.call(kept, unknown); }} catch (e) {{ console.log(\"t\"); }}"
    )
}

const GLTF: &str = "function add(known: any, object: any, def: any) {
    for (const name in def.extensions) {
        if (known[name] === undefined) {
            object.userData.ext = object.userData.ext || {};
            object.userData.ext[name] = def.extensions[name];
        }
    }
}
add({}, obj, j.nodes[0]);";

const LOCAL: &str = "function add(known: any, object: any, def: any) {
    const e = def.extensions;
    for (const name in e) {
        if (known[name] === undefined) {
            object.userData.ext = object.userData.ext || {};
            object.userData.ext[name] = e[name];
        }
    }
}
add({}, obj, j.nodes[0]);";

#[test]
fn gltf_shape_without_extras() {
    let pf = whole(&src(false, GLTF));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn local_receiver_with_extras() {
    let pf = whole(&src(true, LOCAL));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn baseline_no_loop() {
    let pf = whole(&src(true, ""));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Control: an unknown string key on a json value may name an Array method.
#[test]
fn control_unknown_key_escapes() {
    let pf = whole(&src(true, "obj.userData.ext = obj.userData.ext || {}; obj.userData.ext[process.argv[4]] = j.nodes[0].extensions[process.argv[4]];"));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: the parsed value handed to unknown code.
#[test]
fn control_new_function_escapes() {
    let pf = whole(&src(true, &format!("{GLTF} new Function(process.argv[4])()(j);")));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

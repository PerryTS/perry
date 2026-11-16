//! BATCHB5: `o[k]` with `k` the key of `for (k in o)` reads only an
//! enumerable String key of `o` or its chain (ECMA-262 14.7.5.9
//! EnumerateObjectProperties, 14.7.5.10): a JSON value's array chain gives
//! its index keys, never an `Array.prototype` member (every built-in
//! property is non-enumerable, clause 18). three.js GLTFLoader
//! `addUnknownExtensionsToUserData`: `userData.gltfExtensions[name] =
//! objectDef.extensions[name]`; the `library` of Object3D toJSON then held
//! Array.prototype functions and its uuid store escaped (E6).
//! Controls: the same read under the `process.argv[2]` key (an array's
//! `push` is readable there), the subject from `new Function(..)()`, and
//! an enumerable key the program writes onto `Array.prototype`.

use super::box_computed_read::{run, SETUP_ARGV};

fn body(subject: &str, key: &str) -> String {
    format!(
        "{subject}\nconst o: any = {{}};\nfor (const name in s.ext) {{ o[{key}] = s.ext[{key}]; }}\nconst lib: any = o.geometries;\nif (lib) {{ lib[k] = box; }}\nconsole.log(box.witness);"
    )
}

const JSON_S: &str = "const s: any = JSON.parse(k);";
const UNKNOWN_S: &str = "const s: any = new Function(process.argv[2])();";

#[test]
fn a_for_in_key_read_of_a_json_value_reads_no_builtin_member() {
    let (ok, d) = run(SETUP_ARGV, &body(JSON_S, "name"));
    assert!(ok, "{d}");
    let (ok, _) = run(SETUP_ARGV, &body(JSON_S, "k"));
    assert!(!ok, "control: an unknown key may name an Array.prototype method");
    let (ok, _) = run(SETUP_ARGV, &body(UNKNOWN_S, "name"));
    assert!(!ok, "control: unknown subject");
    // The program's own write onto `Array.prototype` makes an enumerable
    // key of every array's chain (10.1.9.2 OrdinarySet: a data property,
    // enumerable): the for-in yields it and the read holds its value.
    let extra = format!(
        "const f: any = new Function(process.argv[2])();\n(Array.prototype as any).extra = f;\n{JSON_S}"
    );
    let (ok, _) = run(SETUP_ARGV, &body(&extra, "name"));
    assert!(!ok, "control: Array.prototype.extra = f is read by the for-in key");
}

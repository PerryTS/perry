use super::detect::{has_top_level_module_exports_assignment, strip_comments_and_strings};
use super::hoist_classes::extract_top_level_class_decls;

#[test]
fn top_level_module_exports_assignment_excludes_nested_factories() {
    let direct =
        strip_comments_and_strings("export default Ajv;\nmodule.exports = exports = Ajv;\n");
    assert!(has_top_level_module_exports_assignment(&direct));

    let nested = strip_comments_and_strings(
        "export function helper() {\n  module.exports = factory();\n}\n",
    );
    assert!(!has_top_level_module_exports_assignment(&nested));

    let read_only = strip_comments_and_strings("export const current = module.exports;\n");
    assert!(!has_top_level_module_exports_assignment(&read_only));
}

#[test]
fn class_hoist_surfaces_later_function_binding() {
    let src = r#"class ParentNode {
  get names() { return addNames({}, this.from); }
}
class CodeGen {
  run() { return new ParentNode().names; }
}
function addNames(names, from) { return names; }
exports.CodeGen = CodeGen;
"#;
    let (blocks, names, rest) = extract_top_level_class_decls(src);
    assert!(
        blocks.starts_with("function addNames"),
        "capture-free helper must be surfaced before classes:\n{blocks}"
    );
    assert!(blocks.contains("class ParentNode"));
    assert!(blocks.contains("class CodeGen"));
    assert_eq!(names, ["ParentNode", "CodeGen"]);
    assert!(!rest.contains("class ParentNode"));
    assert!(!rest.contains("class CodeGen"));
    assert!(rest.contains("function addNames"));
}

#[test]
fn class_stays_wrapped_when_later_function_captures_local() {
    let src = r#"const prefix = "local:";
class Formatter {
  run(value) { return format(value); }
}
function format(value) { return prefix + value; }
exports.Formatter = Formatter;
"#;
    let (blocks, names, rest) = extract_top_level_class_decls(src);
    assert!(
        blocks.is_empty(),
        "capturing helper and dependent class must remain wrapped:\n{blocks}"
    );
    assert!(names.is_empty());
    assert!(rest.contains("class Formatter"));
    assert!(rest.contains("function format"));
}

#[test]
fn class_stays_wrapped_when_later_function_object_is_decorated() {
    let src = r#"class CodeGen {
  run() { return addNames({}).cache.size; }
}
function addNames(names) { return names; }
addNames.cache = new Map();
exports.CodeGen = CodeGen;
"#;
    let (blocks, names, rest) = extract_top_level_class_decls(src);
    assert!(
        blocks.is_empty(),
        "decorated helper and dependent class must remain wrapped:\n{blocks}"
    );
    assert!(names.is_empty());
    assert!(rest.contains("class CodeGen"));
    assert!(rest.contains("addNames.cache = new Map()"));
}

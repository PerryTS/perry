//! `Expr::EnumMember` is its member's value: a number member is a number, a
//! string member a string. As an array index the number is an element read;
//! the string names a key (here an intrinsic method), whose value is a builtin
//! function that ToPrimitive of the holder then calls.
use super::*;

fn holder_escape(index: &str, decl: &str) -> Option<String> {
    let pf = facts(&format!(
        "{decl}
         class Holder {{ x = 1; }}
         const names: any[] = [];
         names[0] = \"zero\"; names[1] = \"one\";
         function nameOf(k: any): any {{ return names[k]; }}
         const holder: any = new Holder();
         const key: string = process.argv[2] ?? \"k\";
         holder[key] = nameOf({index});
         console.log(\"holder=\" + holder);"
    ));
    tmpl(&pf, "Holder").escape.clone()
}

#[test]
fn numeric_member_index_is_an_element_read() {
    let e = holder_escape("Kind.One", "enum Kind { Zero, One, Two }");
    assert_eq!(e, None, "numeric enum member escaped the holder");
}

#[test]
fn literal_index_control_is_an_element_read() {
    assert_eq!(holder_escape("1", "enum Kind { Zero, One, Two }"), None);
}

#[test]
fn string_member_index_is_still_a_string_key() {
    let e = holder_escape("S.A", "enum S { A = \"push\" }");
    assert!(e.is_some(), "a string enum member must stay a string key");
}

/// Whole-program facts of modules as the pipeline prepares them: relative
/// imports resolved, and each module's imported enums fixed up from the
/// declaring module's exported enums (run_pipeline's `fix_imported_enums`).
fn enum_modules(mods: &[(&str, &str)]) -> ProgramFacts {
    let mut hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let exported: Vec<(String, String, Vec<(String, perry_hir::EnumValue)>)> = hirs
        .iter()
        .flat_map(|(p, m)| {
            m.enums.iter().filter(|e| e.is_exported).map(move |e| {
                let members = e.members.iter().map(|x| (x.name.clone(), x.value.clone()));
                (p.clone(), e.name.clone(), members.collect())
            })
        })
        .collect();
    // One level of `export { E as X } from "./m"`, as run_pipeline propagates.
    let mut exported = exported;
    for (p, m) in &hirs {
        for ex in &m.exports {
            if let perry_hir::Export::ReExport {
                source,
                imported,
                exported: name,
            } = ex
            {
                let src = format!("{}.ts", source.trim_start_matches("./"));
                let found: Vec<_> = exported
                    .iter()
                    .filter(|(sp, n, _)| *sp == src && n == imported)
                    .map(|(_, _, mem)| mem.clone())
                    .collect();
                for mem in found {
                    exported.push((p.clone(), name.clone(), mem));
                }
            }
        }
    }
    for (_, m) in &mut hirs {
        let mut imported = std::collections::BTreeMap::new();
        for imp in &m.imports {
            for spec in &imp.specifiers {
                if let perry_hir::ImportSpecifier::Named { imported: i, local } = spec {
                    for (p, n, members) in &exported {
                        if Some(p) == imp.resolved_path.as_ref() && n == i {
                            imported.insert(local.clone(), members.clone());
                        }
                    }
                }
            }
        }
        perry_hir::fix_imported_enums(m, &imported);
    }
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(&list, &FactsConfig::default())
}

const USE: &str = "class Holder { x = 1; }
     const names: any[] = [\"zero\", \"one\"];
     function nameOf(k: any): any { return names[k]; }
     const holder: any = new Holder();
     const key: string = process.argv[2] ?? \"k\";
     holder[key] = nameOf(SK.X);
     console.log(\"holder=\" + holder);";

#[test]
fn an_import_alias_reads_the_declaring_enum_not_an_unrelated_same_named_one() {
    // `SK` is `a.E`, X = 1. Another module declares its own unrelated
    // `enum SK` whose X is the string "push". Reading by the local name finds
    // the string; the import binding says number: an element read.
    let pf = enum_modules(&[
        ("a", "export enum E { X = 1 }"),
        ("other", "export enum SK { X = \"push\" }"),
        (
            "t",
            &format!("import {{ E as SK }} from \"./a\";\nimport \"./other\";\n{USE}"),
        ),
    ]);
    assert_eq!(tmpl(&pf, "Holder").escape, None, "{}", pf.dump_text());
}

#[test]
fn an_aliased_string_member_stays_a_string_key() {
    // `SK` is `a.E`, X = "push" (inlined as a string by the pipeline). An
    // unrelated `enum SK { X = 1 }` elsewhere must not make it a number.
    let pf = enum_modules(&[
        ("a", "export enum E { X = \"push\" }"),
        ("other", "export enum SK { X = 1 }"),
        (
            "t",
            &format!("import {{ E as SK }} from \"./a\";\nimport \"./other\";\n{USE}"),
        ),
    ]);
    assert!(tmpl(&pf, "Holder").escape.is_some(), "{}", pf.dump_text());
}

#[test]
fn an_unresolvable_import_alias_stays_number_or_string() {
    // The binding names a module outside the program: the member is unknown.
    let pf = enum_modules(&[
        ("other", "export enum SK { X = 1 }"),
        (
            "t",
            &format!("import {{ E as SK }} from \"./missing\";\nimport \"./other\";\n{USE}"),
        ),
    ]);
    assert!(tmpl(&pf, "Holder").escape.is_some(), "{}", pf.dump_text());
}

#[test]
fn a_reexported_enum_member_is_read_through_the_barrel() {
    let pf = enum_modules(&[
        ("a", "export enum E { X = 1 }"),
        (
            "barrel",
            "import \"./a\";\nexport { E as SK } from \"./a\";",
        ),
        ("other", "export enum SK { X = \"push\" }"),
        (
            "t",
            &format!("import {{ SK }} from \"./barrel\";\nimport \"./other\";\n{USE}"),
        ),
    ]);
    assert_eq!(tmpl(&pf, "Holder").escape, None, "{}", pf.dump_text());
}

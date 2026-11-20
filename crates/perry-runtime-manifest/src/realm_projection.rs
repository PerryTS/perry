//! Build output that projects a declaration's public metadata back into its
//! existing birth expressions. Each macro expands to a literal; no runtime
//! lookup, metadata object, or catalogue is emitted.
use super::{facts, source_files, tokens, Token};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

type Fields = BTreeMap<String, String>;

fn collect(
    ts: &[Token<'_>],
    declarations: &mut BTreeMap<String, Fields>,
    uses: &mut BTreeSet<(String, String)>,
) -> Result<(), String> {
    for i in 0..ts.len().saturating_sub(4) {
        if ts[i + 1].text == "!" && ts[i + 2].text == "(" && ts[i + 4].text == ")" {
            uses.insert((ts[i].text.into(), ts[i + 3].text.into()));
        }
        let humans = if ts[i].text == "realm_decl" && ts[i + 1].text == "!" && ts[i + 2].text == "{"
        {
            let owned_info = matches!(ts[i + 3].text, "info" | "info_expr");
            if matches!(
                ts[i + 3].text,
                "$" | "birth"
                    | "record"
                    | "object"
                    | "native_nonconstructors"
                    | "native_key"
                    | "module_provider"
            ) {
                continue;
            }
            let mut depth = 1;
            let mut end = i + 3;
            while depth != 0 {
                match ts
                    .get(end)
                    .ok_or("unterminated realm projection block")?
                    .text
                {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            let entries = if owned_info {
                let at = ts[i + 3..end - 1]
                    .windows(2)
                    .position(|w| w[0].text == "realm" && w[1].text == "{")
                    .ok_or("owned info requires its realm metadata")?
                    + i
                    + 3;
                &ts[at + 2..end - 2]
            } else {
                &ts[i + 3..end - 1]
            };
            super::info::split(entries, ";")
                .into_iter()
                .map(facts::parse)
                .collect::<Result<Vec<_>, _>>()?
        } else {
            if ts[i].text != "realm"
                || i < 2
                || ts[i - 1].text != "["
                || ts[i - 2].text != "#"
                || ts[i + 1].text != "("
            {
                continue;
            }
            if ts[i + 2].text == "$" {
                continue;
            }
            let end = super::closing_paren(ts, i + 1)?;
            facts::parse_many(&ts[i + 2..end])?
        };
        for facts in humans {
            let Some(name) = facts.metadata_macro else {
                continue;
            };
            let mut fields = Fields::new();
            if let Some(length) = facts.spec_length {
                fields.insert("length".into(), length.to_string());
            }
            if let Some(label) = facts.spec_name {
                fields.insert("name".into(), Value::String(label).encode());
            }
            if fields.is_empty() {
                return Err(format!(
                    "{name}: metadata projection needs a nondefault fact"
                ));
            }
            if let Some(previous) = declarations.insert(name.clone(), fields.clone()) {
                if previous != fields {
                    return Err(format!("{name}: shared projection has conflicting facts"));
                }
            }
        }
    }
    Ok(())
}

fn render(
    declarations: &BTreeMap<String, Fields>,
    uses: &BTreeSet<(String, String)>,
) -> Result<String, String> {
    let mut output = String::from("// Generated from realm declarations. Do not edit.\n");
    for (name, fields) in declarations {
        output.push_str(&format!("#[macro_export]\nmacro_rules! {name} {{\n"));
        for (field, literal) in fields {
            if !uses.contains(&(name.clone(), field.clone())) {
                return Err(format!(
                    "{name}: public {field} is not consumed by a runtime birth"
                ));
            }
            output.push_str(&format!("    ({field}) => {{ {literal} }};\n"));
        }
        output.push_str("}\n");
    }
    Ok(output)
}

pub(super) fn generate(runtime_src: &Path, out: &Path) -> Result<(), String> {
    let runtime_src = runtime_src.canonicalize().map_err(|e| e.to_string())?;
    let crates = runtime_src
        .parent()
        .and_then(Path::parent)
        .ok_or("missing crates root")?;
    let mut declarations = BTreeMap::new();
    let mut uses = BTreeSet::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates).map_err(|e| e.to_string())? {
        let src = entry.map_err(|e| e.to_string())?.path().join("src");
        if src.is_dir() {
            source_files(&src, &mut files).map_err(|e| e.to_string())?;
        }
    }
    for file in files {
        let source = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        if !source.contains("realm_") && !source.contains("#[realm") {
            continue;
        }
        collect(&tokens(&source)?, &mut declarations, &mut uses)?;
    }
    let output = render(&declarations, &uses)?;
    std::fs::write(out.join("realm-metadata.rs"), output).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_info_initializers_project_metadata_without_changing_static_identity() {
        let mut declarations = BTreeMap::new();
        let mut uses = BTreeSet::new();
        let source = r#"static INFO:JsFunctionInfo=realm_decl! { info_expr JsFunctionInfo::of(body as JsBody3); realm { Example.method,spec={length=2},emit=metadata(realm_method); } }; let length=realm_method!(length);"#;
        collect(&tokens(source).unwrap(), &mut declarations, &mut uses).unwrap();
        assert!(render(&declarations, &uses)
            .unwrap()
            .contains("(length) => { 2 }"));
    }

    #[test]
    fn projections_are_literals_and_must_be_consumed() {
        let mut declarations = BTreeMap::new();
        let mut uses = BTreeSet::new();
        collect(
            &tokens("fn_info!(#[realm(Foo, spec={length=1}, emit=metadata(realm_foo))] body,3);")
                .unwrap(),
            &mut declarations,
            &mut uses,
        )
        .unwrap();
        assert!(render(&declarations, &uses).is_err());
        collect(
            &tokens("let length = realm_foo!(length);").unwrap(),
            &mut declarations,
            &mut uses,
        )
        .unwrap();
        let output = render(&declarations, &uses).unwrap();
        assert!(output.contains("(length) => { 1 }"));
        assert!(!output.contains("static"));
        assert!(collect(
            &tokens("fn_info!(#[realm(Bar, spec={length=2}, emit=metadata(realm_foo))] other,3);")
                .unwrap(),
            &mut declarations,
            &mut uses
        )
        .is_err());
    }
    #[test]
    fn native_block_overrides_project_to_runtime_literals() {
        let mut declarations = BTreeMap::new();
        let mut uses = BTreeSet::new();
        collect(&tokens(r#"realm_decl! { "node:example.method",provider=bound_method,receiver="node:example",spec={length=2},emit=metadata(realm_native_example); } let arity=realm_native_example!(length);"#).unwrap(),&mut declarations,&mut uses).unwrap();
        assert!(render(&declarations, &uses)
            .unwrap()
            .contains("(length) => { 2 }"));
    }
}

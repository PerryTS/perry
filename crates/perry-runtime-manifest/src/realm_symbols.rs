//! Transient source symbol resolution. This is built from Rust modules on each
//! run; no runtime symbol catalogue or reflected symbol strings are inputs.
use super::{tokens, Token};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) struct Sources {
    symbols: BTreeMap<String, Vec<Symbol>>,
    modules: BTreeMap<PathBuf, String>,
    imports: super::imports::Imports,
}

struct Symbol {
    rust: String,
    export: Option<String>,
    file: PathBuf,
}

fn matching(ts: &[Token<'_>], open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, token) in ts.iter().enumerate().skip(open) {
        if token.text == "{" {
            depth += 1;
        }
        if token.text == "}" {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

fn module_edges(
    ts: &[Token<'_>],
    file: &Path,
    prefix: &str,
    module_dir: &Path,
    available: &BTreeSet<PathBuf>,
    edges: &mut Vec<(PathBuf, String)>,
) {
    let mut i = 0;
    let mut path_attribute = None;
    while i < ts.len() {
        if ts[i].text == "#" && ts.get(i + 2).is_some_and(|t| t.text == "path") {
            path_attribute = ts.get(i + 4).and_then(|t| t.string.as_deref());
        }
        if ts[i].text == "mod" && i + 2 < ts.len() {
            let name = ts[i + 1].text;
            let child_prefix = format!("{prefix}::{name}");
            if ts[i + 2].text == "{" {
                if let Some(end) = matching(ts, i + 2) {
                    module_edges(
                        &ts[i + 3..end],
                        file,
                        &child_prefix,
                        &module_dir.join(name),
                        available,
                        edges,
                    );
                    i = end + 1;
                    path_attribute = None;
                    continue;
                }
            } else if ts[i + 2].text == ";" {
                let candidates = if let Some(path) = path_attribute {
                    vec![file.parent().unwrap().join(path)]
                } else {
                    vec![
                        module_dir.join(format!("{name}.rs")),
                        module_dir.join(name).join("mod.rs"),
                    ]
                };
                for path in candidates {
                    if available.contains(&path) {
                        edges.push((path, child_prefix.clone()));
                    }
                }
                path_attribute = None;
            }
        } else if ts[i].text == "{" {
            // An item body or macro definition cannot declare a sibling module.
            if let Some(end) = matching(ts, i) {
                i = end + 1;
                path_attribute = None;
                continue;
            }
        } else if ts[i].text == ";" {
            path_attribute = None;
        }
        i += 1;
    }
}

fn generated_functions(
    ts: &[Token<'_>],
    file: &Path,
    prefix: &str,
    symbols: &mut BTreeMap<String, Vec<Symbol>>,
) {
    // A module-local macro may declare a function from an identifier argument.
    // Read that binding from its matcher, not from a catalogue of macro names.
    enum Binding {
        Argument(usize),
        Repeated(Vec<String>),
    }
    let mut templates = BTreeMap::new();
    for i in 0..ts.len().saturating_sub(4) {
        if ts[i].text != "macro_rules" || ts[i + 1].text != "!" || ts[i + 3].text != "{" {
            continue;
        }
        let Some(end) = matching(ts, i + 3) else {
            continue;
        };
        let inner = &ts[i + 4..end];
        let Ok(pattern_end) = super::closing_paren(inner, 0) else {
            continue;
        };
        let arrow = pattern_end + 1;
        if inner.get(arrow).is_none_or(|t| t.text != "=")
            || inner.get(arrow + 1).is_none_or(|t| t.text != ">")
        {
            continue;
        }
        let pattern = &inner[..arrow];
        if pattern.first().is_none_or(|t| t.text != "(")
            || pattern.last().is_none_or(|t| t.text != ")")
        {
            continue;
        }
        let args = super::info::split(&pattern[1..pattern.len() - 1], ",");
        let mut bodies = Vec::new();
        for j in arrow + 2..inner.len().saturating_sub(2) {
            if inner[j].text != "fn" || inner[j + 1].text != "$" {
                continue;
            }
            let parameter = inner[j + 2].text;
            let matcher = &pattern[1..pattern.len() - 1];
            let binding = if matcher.len() >= 6
                && matcher[0].text == "$"
                && matcher[1].text == "("
                && matcher[2].text == "$"
                && matcher[3].text == parameter
                && matcher[4].text == ":"
                && matcher[5].text == "ident"
            {
                let suffix = matcher[6..]
                    .iter()
                    .take_while(|t| !matches!(t.text, "$" | "(" | ")" | "[" | "]" | "{" | "}"))
                    .map(|t| t.text.to_owned())
                    .collect();
                Some(Binding::Repeated(suffix))
            } else {
                args.iter()
                    .position(|arg| {
                        arg.windows(4).any(|w| {
                            w[0].text == "$"
                                && w[1].text == parameter
                                && w[2].text == ":"
                                && w[3].text == "ident"
                        })
                    })
                    .map(Binding::Argument)
            };
            if let Some(binding) = binding {
                bodies.push((
                    binding,
                    inner[arrow + 2..j].iter().any(|t| t.text == "no_mangle"),
                ));
            }
        }
        if !bodies.is_empty() {
            templates.insert(ts[i + 2].text, bodies);
        }
    }
    for i in 0..ts.len().saturating_sub(3) {
        let Some(bodies) = templates.get(ts[i].text) else {
            continue;
        };
        if ts[i + 1].text != "!" || !matches!(ts[i + 2].text, "(" | "{") {
            continue;
        }
        let end = if ts[i + 2].text == "(" {
            super::closing_paren(ts, i + 2).ok()
        } else {
            matching(ts, i + 2)
        };
        let Some(end) = end else { continue };
        let input = &ts[i + 3..end];
        for (binding, exported) in bodies {
            let arguments = super::info::split(input, ",");
            let names = match binding {
                Binding::Argument(index) => arguments
                    .get(*index)
                    .into_iter()
                    .filter(|arg| arg.len() == 1)
                    .map(|arg| &arg[0])
                    .collect::<Vec<_>>(),
                Binding::Repeated(suffix) => super::info::split(input, ";")
                    .into_iter()
                    .flat_map(|row| super::info::split(row, ","))
                    .filter(|row| {
                        !row.is_empty()
                            && row.len() > suffix.len()
                            && suffix.iter().enumerate().all(|(i, t)| row[i + 1].text == t)
                    })
                    .map(|row| &row[0])
                    .collect(),
            };
            for name in names {
                if name.string.is_some() || name.text == "$" {
                    continue;
                }
                symbols.entry(name.text.into()).or_default().push(Symbol {
                    rust: format!("{prefix}::{}", name.text),
                    export: exported.then(|| name.text.into()),
                    file: file.to_owned(),
                });
            }
        }
    }
}

fn definitions(
    ts: &[Token<'_>],
    file: &Path,
    prefix: &str,
    symbols: &mut BTreeMap<String, Vec<Symbol>>,
) {
    generated_functions(ts, file, prefix, symbols);
    let mut i = 0;
    let mut item_start = 0;
    while i < ts.len() {
        if ts[i].text == "realm_decl"
            && ts.get(i + 1).is_some_and(|t| t.text == "!")
            && ts.get(i + 2).is_some_and(|t| t.text == "{")
            && ts.get(i + 3).is_some_and(|t| t.text == "module_provider")
        {
            if let Some(end) = matching(ts, i + 2) {
                definitions(&ts[i + 4..end], file, prefix, symbols);
                i = end + 1;
                item_start = i;
                continue;
            }
        }
        if ts[i].text == "mod" && ts.get(i + 2).is_some_and(|t| t.text == "{") {
            if let Some(end) = matching(ts, i + 2) {
                definitions(
                    &ts[i + 3..end],
                    file,
                    &format!("{prefix}::{}", ts[i + 1].text),
                    symbols,
                );
                i = end + 1;
                item_start = i;
                continue;
            }
        }
        if ts[i].text == "fn" && ts.get(i + 2).is_some_and(|t| t.text == "(") {
            let name = ts[i + 1].text;
            if name != "$" {
                let attrs = &ts[item_start..i];
                let export = if attrs.iter().any(|t| t.text == "no_mangle") {
                    Some(name.to_owned())
                } else {
                    attrs
                        .iter()
                        .position(|t| t.text == "export_name")
                        .and_then(|j| attrs.get(j + 2))
                        .and_then(|t| t.string.clone())
                };
                symbols.entry(name.to_owned()).or_default().push(Symbol {
                    rust: format!("{prefix}::{name}"),
                    export,
                    file: file.to_owned(),
                });
            }
        }
        if ts[i].text == "{" {
            if let Some(end) = matching(ts, i) {
                i = end + 1;
                item_start = i;
                continue;
            }
        }
        if ts[i].text == ";" {
            item_start = i + 1;
        }
        i += 1;
    }
}

impl Sources {
    pub fn new(files: &[(PathBuf, String)], crates: &Path) -> Result<Self, String> {
        let available = files
            .iter()
            .map(|(p, _)| p.clone())
            .collect::<BTreeSet<_>>();
        let mut modules = BTreeMap::new();
        let mut parsed = BTreeMap::new();
        for (file, source) in files {
            if file.file_name().is_some_and(|name| name == "lib.rs")
                && file
                    .parent()
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == "src"))
            {
                let package = file
                    .strip_prefix(crates)
                    .map_err(|e| e.to_string())?
                    .components()
                    .next()
                    .unwrap()
                    .as_os_str()
                    .to_string_lossy()
                    .replace('-', "_");
                modules.insert(file.clone(), package);
            }
            parsed.insert(file.clone(), tokens(source)?);
        }
        let mut visited = BTreeSet::new();
        loop {
            let todo = modules
                .iter()
                .filter(|(p, _)| !visited.contains(*p))
                .map(|(p, m)| (p.clone(), m.clone()))
                .collect::<Vec<_>>();
            if todo.is_empty() {
                break;
            }
            for (file, prefix) in todo {
                visited.insert(file.clone());
                let Some(ts) = parsed.get(&file) else {
                    continue;
                };
                let filename = file.file_name().unwrap().to_string_lossy();
                let dir = if filename == "lib.rs" || filename == "mod.rs" {
                    file.parent().unwrap().to_owned()
                } else {
                    file.with_extension("")
                };
                let mut edges = Vec::new();
                module_edges(ts, &file, &prefix, &dir, &available, &mut edges);
                for (child, name) in edges {
                    modules.entry(child).or_insert(name);
                }
            }
        }
        let mut symbols = BTreeMap::new();
        for (file, prefix) in &modules {
            if let Some(ts) = parsed.get(file) {
                definitions(ts, file, prefix, &mut symbols);
            }
        }
        let mut imports = super::imports::Imports::default();
        for (file, source) in files {
            if let Some(module) = modules.get(file) {
                imports.add(module, source)?;
            }
        }
        Ok(Self {
            symbols,
            modules,
            imports,
        })
    }

    pub fn provider_body(&self, name: &str, source: &Path) -> Result<String, String> {
        let matches = self
            .symbols
            .get(name)
            .into_iter()
            .flatten()
            .filter(|symbol| symbol.file == source)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!("module provider {name} must own one function item"));
        }
        let rust = &matches[0].rust;
        Ok(rust
            .strip_prefix("perry_runtime::")
            .map_or_else(|| rust.clone(), |tail| format!("crate::{tail}")))
    }

    pub fn resolve(&self, reference: &str, source: &Path) -> Result<(String, String), String> {
        let module = self
            .modules
            .get(source)
            .ok_or_else(|| format!("unknown Rust module: {}", source.display()))?;
        let package = module.split("::").next().unwrap();
        let reference = reference.replace(' ', "");
        let mut expanded = if let Some(rest) = reference.strip_prefix("crate::") {
            format!("{package}::{rest}")
        } else if let Some(rest) = reference.strip_prefix("self::") {
            format!("{module}::{rest}")
        } else if reference.starts_with("super::") {
            let mut base = module.as_str();
            let mut rest = reference.as_str();
            while let Some(tail) = rest.strip_prefix("super::") {
                base = base.rsplit_once("::").ok_or("super path escapes crate")?.0;
                rest = tail;
            }
            format!("{base}::{rest}")
        } else if reference.starts_with("perry_") {
            reference.clone()
        } else {
            format!("{module}::{reference}")
        };
        if !expanded.contains("::") {
            expanded = format!("{module}::{expanded}");
        }
        let leaf = reference.rsplit("::").next().unwrap();
        let candidates = self
            .symbols
            .get(leaf)
            .ok_or_else(|| format!("missing source body {reference}"))?;
        let suffix = format!("::{reference}");
        let matching_suffix = candidates
            .iter()
            .filter(|s| s.rust.ends_with(&suffix))
            .collect::<Vec<_>>();
        let selected = candidates
            .iter()
            .find(|s| s.rust == expanded)
            .or_else(|| {
                let expanded_paths = self.imports.expand(&expanded);
                candidates.iter().find(|s| expanded_paths.contains(&s.rust))
            })
            .or_else(|| (matching_suffix.len() == 1).then(|| matching_suffix[0]))
            .or_else(|| candidates.iter().find(|s| s.file == source))
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
            .ok_or_else(|| format!("ambiguous source body {reference} in {}", source.display()))?;
        let symbol = selected
            .export
            .clone()
            .unwrap_or_else(|| selected.rust.clone());
        let body = symbol
            .strip_prefix("perry_runtime::")
            .map_or_else(|| symbol.clone(), |tail| format!("crate::{tail}"));
        Ok((body, format!("source:{symbol}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_provider_keeps_rust_identity_and_exported_symbol() {
        let path = PathBuf::from("/crates/perry-runtime/src/lib.rs");
        let files=vec![(path.clone(),r#"realm_decl! { module_provider #[no_mangle] pub extern "C" fn provider() {} realm { "node:sample", members={}; } }"#.into())];
        let sources = Sources::new(&files, Path::new("/crates")).unwrap();
        assert_eq!(
            sources.provider_body("provider", &path).unwrap(),
            "crate::provider"
        );
        assert_eq!(sources.resolve("provider", &path).unwrap().0, "provider");
    }

    #[test]
    fn repeated_macro_rows_define_their_declared_body_identifiers() {
        let files = vec![
            (PathBuf::from("/crates/perry-runtime/src/lib.rs"), "mod generated; mod caller;".into()),
            (PathBuf::from("/crates/perry-runtime/src/generated.rs"),
             "macro_rules! methods { ($($body:ident = $name:literal ($($arg:ident),*);)*) => { $(fn $body() {})* }; }
              methods! { append = \"append\"(a,b); clear = \"clear\"(); }".into()),
            (PathBuf::from("/crates/perry-runtime/src/caller.rs"), "use crate::generated as alias;".into()),
        ];
        let sources = Sources::new(&files, Path::new("/crates")).unwrap();
        assert_eq!(
            sources.resolve("alias::append", &files[2].0).unwrap().0,
            "crate::generated::append"
        );
        assert_eq!(
            sources.resolve("alias::clear", &files[2].0).unwrap().0,
            "crate::generated::clear"
        );
        assert!(sources.resolve("alias::a", &files[2].0).is_err());
    }

    #[test]
    fn resolves_exports_imported_bodies_and_path_modules() {
        let files=vec![
            (PathBuf::from("/crates/perry-runtime/src/lib.rs"), "#[path=\"shifted.rs\"] mod actual; mod plain;".into()),
            (PathBuf::from("/crates/perry-runtime/src/shifted.rs"), "pub fn imported() {} #[no_mangle] pub extern \"C\" fn exported() {} mod inline { fn child() {} }".into()),
            (PathBuf::from("/crates/perry-runtime/src/plain.rs"), "fn caller() {}".into()),
        ];
        let sources = Sources::new(&files, Path::new("/crates")).unwrap();
        let from = &files[2].0;
        assert_eq!(
            sources.resolve("imported", from).unwrap(),
            (
                "crate::actual::imported".into(),
                "source:perry_runtime::actual::imported".into()
            )
        );
        assert_eq!(
            sources.resolve("crate::actual::exported", from).unwrap(),
            ("exported".into(), "source:exported".into())
        );
        assert_eq!(
            sources
                .resolve("crate::actual::inline::child", from)
                .unwrap()
                .0,
            "crate::actual::inline::child"
        );
    }
}

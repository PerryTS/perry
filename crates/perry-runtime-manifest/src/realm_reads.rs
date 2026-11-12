//! Runtime value-read projection, from the declaration beside each raw read.
use super::{info, tokens, Token};
use crate::realm_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const RAW: [&str; 4] = [
    "builtin_prototype_value",
    "builtin_constructor_value",
    "intrinsic_prototype_addr",
    "intrinsic_constructor_addr",
];
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Read {
    Cell(String, String),
    Table { prototype: bool },
}

fn closing(ts: &[Token<'_>], open: usize) -> Result<usize, String> {
    let close = match ts[open].text {
        "{" => "}",
        "(" => ")",
        "[" => "]",
        _ => return Err("expected an opening delimiter".into()),
    };
    let mut depth = 0;
    for i in open..ts.len() {
        if ts[i].text == ts[open].text {
            depth += 1;
        }
        if ts[i].text == close {
            depth -= 1;
            if depth == 0 {
                return Ok(i);
            }
        }
    }
    Err("unterminated runtime read".into())
}

pub(super) fn private_bindings(source: &str) -> Result<BTreeSet<String>, String> {
    let ts = tokens(source)?;
    let mut names = BTreeSet::new();
    for i in 0..ts.len() {
        if ts[i].text != "IntrinsicPrivateReadSite" {
            continue;
        }
        if let Some(j) = (0..i)
            .rev()
            .take_while(|&j| !matches!(ts[j].text, ";" | "{" | "}"))
            .find(|&j| matches!(ts[j].text, "static" | "let"))
        {
            let j = j + 1 + usize::from(ts.get(j + 1).is_some_and(|t| t.text == "mut"));
            names.insert(ts[j].text.to_owned());
        }
    }
    Ok(names)
}

fn test_source(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "tests")
        || path.file_stem().is_some_and(|s| {
            let s = s.to_string_lossy();
            s == "test" || s.contains("tests")
        })
}
fn excluded(ts: &[Token<'_>]) -> Result<Vec<bool>, String> {
    let mut skip = vec![false; ts.len()];
    for i in 0..ts.len() {
        let raw_body = ts[i].text == "fn" && ts.get(i + 1).is_some_and(|t| RAW.contains(&t.text));
        let test = ts[i].text == "#"
            && ts.get(i + 1).is_some_and(|t| t.text == "[")
            && (ts.get(i + 2).is_some_and(|t| t.text == "test")
                || (ts.get(i + 2).is_some_and(|t| t.text == "cfg")
                    && ts.get(i + 4).is_some_and(|t| t.text == "test")));
        if raw_body || test {
            if let Some(open) = (i..ts.len()).find(|&j| matches!(ts[j].text, "{" | ";")) {
                let end = if ts[open].text == ";" {
                    open
                } else {
                    closing(ts, open)?
                };
                skip[i..=end].fill(true);
            }
        }
    }
    Ok(skip)
}

pub(super) fn collect_at(
    path: &Path,
    source: &str,
    private: &BTreeSet<String>,
) -> Result<Vec<Read>, String> {
    if test_source(path) {
        return Ok(Vec::new());
    }
    let ts = tokens(source)?;
    let mut skip = excluded(&ts)?;
    let mut result = Vec::new();
    let line = |i: usize| {
        source[..ts[i].start]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1
    };
    let fail = |i, why: &str| format!("{}:{}: {why}", path.display(), line(i));
    for i in 0..ts.len().saturating_sub(2) {
        if skip[i] || ts[i].text != "realm_value_read" || ts[i + 1].text != "!" {
            continue;
        }
        if ts[i + 2].text != "{" {
            return Err(fail(i, "realm_value_read needs braces"));
        }
        let end = closing(&ts, i + 2)?;
        if ts.get(i + 3).is_some_and(|t| t.text == "table") {
            if ts.get(i + 4).map(|t| t.text) != Some("=")
                || ts.get(i + 5).map(|t| t.text) != Some("GLOBAL_THIS_BUILTIN_CONSTRUCTORS")
                || ts.get(i + 6).map(|t| t.text) != Some(";")
            {
                return Err(fail(
                    i,
                    "runtime read table must be GLOBAL_THIS_BUILTIN_CONSTRUCTORS",
                ));
            }
            let raw: BTreeSet<_> = ts[i + 7..end]
                .iter()
                .filter(|t| RAW.contains(&t.text))
                .map(|t| t.text.contains("prototype"))
                .collect();
            if raw.len() != 1 {
                return Err(fail(
                    i,
                    "table read needs one constructor or prototype accessor kind",
                ));
            }
            result.push(Read::Table {
                prototype: *raw.first().unwrap(),
            });
        } else {
            let subject = ts.get(i + 3).and_then(|t| t.string.clone());
            let target = ts.get(i + 6).and_then(|t| t.string.clone());
            if subject.is_none()
                || target.is_none()
                || ts.get(i + 4).map(|t| t.text) != Some("=")
                || ts.get(i + 5).map(|t| t.text) != Some(">")
                || ts.get(i + 7).map(|t| t.text) != Some(";")
            {
                return Err(fail(i, "runtime read needs subject => target; expression"));
            }
            let (subject, target) = (subject.unwrap(), target.unwrap());
            if [subject.as_str(), target.as_str()]
                .iter()
                .any(|s| s.is_empty() || *s == "*")
            {
                return Err(fail(
                    i,
                    "runtime read names must be literal cells, never a wildcard",
                ));
            }
            result.push(Read::Cell(subject, target));
        }
        skip[i..=end].fill(true);
    }
    let mut aliases: BTreeMap<String, String> =
        RAW.iter().map(|s| ((*s).into(), (*s).into())).collect();
    for i in 0..ts.len() {
        if let Some(raw) = aliases.get(ts[i].text).cloned() {
            if ts.get(i + 1).map(|t| t.text) == Some("as") {
                if let Some(name) = ts.get(i + 2) {
                    aliases.insert(name.text.into(), raw.clone());
                }
            }
            if i >= 2 && ts[i - 1].text == "=" {
                aliases.insert(ts[i - 2].text.into(), raw);
            }
        }
    }
    for i in 0..ts.len().saturating_sub(1) {
        if skip[i] || ts[i + 1].text != "(" {
            continue;
        }
        let raw = aliases.contains_key(ts[i].text) && (i == 0 || ts[i - 1].text != "fn");
        let private_read = ts[i].text == "read"
            && i >= 2
            && ts[i - 1].text == "."
            && private.contains(ts[i - 2].text);
        let private_ufcs = ts[i].text == "read"
            && i >= 3
            && ts[i - 1].text == ":"
            && ts[i - 2].text == ":"
            && ts[i - 3].text == "IntrinsicPrivateReadSite";
        if raw || private_read || private_ufcs {
            return Err(fail(
                i,
                &format!(
                    "undeclared runtime value read of {}; use realm_value_read!",
                    ts[i].text
                ),
            ));
        }
    }
    Ok(result)
}

/// Names come from the same source table that generates the runtime identities.
pub(super) fn constructor_names(source: &str) -> Result<BTreeSet<String>, String> {
    let ts = tokens(source)?;
    let mut names = BTreeSet::new();
    for i in 0..ts.len().saturating_sub(2) {
        if ts[i].text == "builtin_constructors" && ts[i + 1].text == "!" && ts[i + 2].text == "{" {
            let end = closing(&ts, i + 2)?;
            for row in info::split(&ts[i + 3..end], ",") {
                if row.is_empty() {
                    continue;
                }
                if row.get(1).map(|t| t.text) != Some("=")
                    || row.get(2).map(|t| t.text) != Some(">")
                {
                    return Err("invalid builtin constructor table row".into());
                }
                names.insert(row[0].text.into());
            }
        }
    }
    Ok(names)
}

pub(super) fn attach(
    all: &mut [Value],
    reads: &[Read],
    table: &BTreeSet<String>,
) -> Result<(), String> {
    let names: BTreeMap<_, _> = all
        .iter()
        .enumerate()
        .filter_map(|(i, d)| Some((d.get("name")?.string()?.to_owned(), i)))
        .collect();
    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for read in reads {
        match read {
            Read::Cell(k, c) => {
                edges.insert((k.clone(), c.clone()));
            }
            Read::Table { prototype } => {
                if table.is_empty() {
                    return Err("runtime read constructor table is empty".into());
                }
                // The runtime table supplies names; the authoritative shape
                // supplies subjects. Only declared subjects can belong to R.
                for (name, &i) in names.iter().filter(|(name, _)| table.contains(*name)) {
                    let target = if *prototype {
                        // A row without an instance-prototype slot reads no cell
                        // The shape supplies the target.
                        all[i]
                            .get("properties")
                            .and_then(Value::array)
                            .into_iter()
                            .flatten()
                            .find(|p| p.get("key").and_then(Value::string) == Some("prototype"))
                            .and_then(|p| p.get("value"))
                            .filter(|v| v.get("kind").and_then(Value::string) == Some("reference"))
                            .and_then(|v| v.get("value"))
                            .and_then(Value::string)
                            .map(str::to_owned)
                    } else {
                        Some(name.clone())
                    };
                    if let Some(target) = target {
                        edges.insert((name.clone(), target));
                    }
                }
            }
        }
    }
    for (subject, target) in edges {
        let i = *names
            .get(&subject)
            .ok_or_else(|| format!("runtime read subject {subject} has no declaration"))?;
        if !names.contains_key(&target) {
            return Err(format!("runtime read target {target} has no declaration"));
        }
        let Value::Object(d) = &mut all[i] else {
            unreachable!()
        };
        let values = d
            .entry("runtime_reads".into())
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(values) = values else {
            return Err("runtime_reads must be an array".into());
        };
        if !values.iter().any(|v| v.string() == Some(&target)) {
            values.push(Value::String(target));
        }
        values.sort_by(|a, b| a.string().cmp(&b.string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn collect(s: &str) -> Result<Vec<Read>, String> {
        collect_at(Path::new("runtime.rs"), s, &private_bindings(s)?)
    }
    #[test]
    fn literal_pairs_are_projected_by_subject() {
        assert_eq!(collect(r#"crate::realm_value_read! { "Date" => "Date.prototype"; builtin_prototype_value("Date") }"#).unwrap(), vec![Read::Cell("Date".into(),"Date.prototype".into())]);
    }
    #[test]
    fn raw_read_reports_file_and_line() {
        let e = collect("fn f() {\n builtin_prototype_value(\"Date\"); }").unwrap_err();
        assert!(e.contains("runtime.rs:2:"), "{e}");
        assert!(collect(
            "use x::builtin_constructor_value as read_ctor; fn f(){ read_ctor(\"Date\"); }"
        )
        .is_err());
    }
    #[test]
    fn private_site_requires_a_wrapper() {
        let s="static P: IntrinsicPrivateReadSite = IntrinsicPrivateReadSite::new(&A); fn f(){ P.read(g); }";
        assert!(collect(s).unwrap_err().contains("runtime.rs:1:"));
        assert!(collect("fn f() { IntrinsicPrivateReadSite::read(&P,g); }").is_err());
        assert!(collect(&s.replace(
            "P.read(g)",
            "crate::realm_value_read! { \"RegExp\" => \"RegExp.prototype\"; P.read(g) }"
        ))
        .is_ok());
    }
    #[test]
    fn lexer_ignores_comments_strings_tests_and_accessor_definitions() {
        assert!(collect(
            r###"// builtin_constructor_value("Fake")
 const X: &str = r#"builtin_prototype_value("Fake")"#;
 #[cfg(test)] mod tests { fn f(){ builtin_prototype_value("Date"); } }
 fn builtin_prototype_value(name: &str) { intrinsic_prototype_addr(name); }
 "###
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn nonliteral_reads_use_the_runtime_table_and_never_a_wildcard() {
        assert_eq!(collect("realm_value_read! { table = GLOBAL_THIS_BUILTIN_CONSTRUCTORS; builtin_prototype_value(name) }").unwrap(),vec![Read::Table{prototype:true}]);
        assert!(collect(r#"realm_value_read! { "*" => "Date.prototype"; f() }"#).is_err());
        assert_eq!(
            constructor_names("builtin_constructors!{ Date => f!(1,2); class C, RegExp => g, }")
                .unwrap(),
            ["Date".into(), "RegExp".into()].into()
        );
    }
    #[test]
    fn attach_changes_only_the_subjects_read_fact() {
        fn row(n: &str) -> Value {
            Value::Object(BTreeMap::from([("name".into(), Value::String(n.into()))]))
        }
        let mut all = [
            row("Date"),
            row("Date.prototype"),
            row("RegExp"),
            row("RegExp.prototype"),
        ];
        attach(
            &mut all,
            &[
                Read::Cell("Date".into(), "Date.prototype".into()),
                Read::Table { prototype: false },
            ],
            &["RegExp".into()].into(),
        )
        .unwrap();
        assert!(all[0].get("runtime_reads").is_some());
        assert!(all[1].get("runtime_reads").is_none());
        assert!(all.iter().all(|d| d.get("births").is_none()));
        assert!(attach(
            &mut all,
            &[Read::Cell("Missing".into(), "Date".into())],
            &BTreeSet::new()
        )
        .is_err());
    }
    #[test]
    fn table_reads_join_declared_subjects_and_their_prototype_slots() {
        let mut all = [
            Value::Object(BTreeMap::from([
                ("name".into(), Value::String("Thing".into())),
                (
                    "properties".into(),
                    Value::Array(vec![Value::Object(BTreeMap::from([
                        ("key".into(), Value::String("prototype".into())),
                        (
                            "value".into(),
                            Value::Object(BTreeMap::from([
                                ("kind".into(), Value::String("reference".into())),
                                ("value".into(), Value::String("%SharedProto%".into())),
                            ])),
                        ),
                    ]))]),
                ),
            ])),
            Value::Object(BTreeMap::from([(
                "name".into(),
                Value::String("%SharedProto%".into()),
            )])),
        ];
        attach(
            &mut all,
            &[Read::Table { prototype: true }],
            &["Thing".into(), "RuntimeOnly".into()].into(),
        )
        .unwrap();
        assert_eq!(
            all[0].get("runtime_reads").unwrap().array().unwrap()[0].string(),
            Some("%SharedProto%")
        );
    }
}

#[cfg(feature = "analyzer")]
use crate::{
    graph::{crate_of, temp_label},
    instructions::rx,
};
use anyhow::{bail, Context, Result};
#[cfg(feature = "analyzer")]
use std::collections::HashSet;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};
#[cfg(feature = "analyzer")]
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Leaf,
    AllocOnly,
    ThrowOnly,
    Reenters,
}
impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Leaf => "Leaf",
            Self::AllocOnly => "AllocOnly",
            Self::ThrowOnly => "ThrowOnly",
            Self::Reenters => "Reenters",
        })
    }
}
impl FromStr for Class {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "Leaf" => Self::Leaf,
            "AllocOnly" => Self::AllocOnly,
            "ThrowOnly" => Self::ThrowOnly,
            "Reenters" => Self::Reenters,
            _ => bail!("invalid class {s}"),
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub class: Class,
    pub throws: bool,
}
impl Entry {
    pub fn throws_name(self) -> &'static str {
        if self.throws {
            "yes"
        } else {
            "no"
        }
    }
}
pub type Table = BTreeMap<String, Entry>;

#[cfg(feature = "analyzer")]
pub(crate) fn native_stems(root: &Path) -> Result<HashSet<String>> {
    let mut stems = HashSet::new();
    for krate in ["perry-runtime", "perry-stdlib"] {
        let dir = root.join("crates").join(krate).join("src");
        if !dir.is_dir() {
            continue;
        }
        for p in WalkDir::new(dir) {
            let p = p?;
            if p.file_type().is_file()
                && p.path()
                    .extension()
                    .is_some_and(|x| ["c", "cc", "cpp", "S"].iter().any(|y| x == *y))
            {
                if let Some(stem) = p.path().file_stem() {
                    stems.insert(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    Ok(stems)
}
#[cfg(feature = "analyzer")]
pub(crate) fn perry_member(obj: &str, name: &str, stems: &HashSet<String>) -> bool {
    if let Some(krate) = crate_of(obj) {
        if !krate.starts_with("perry") {
            return false;
        }
    } else {
        let member = obj.rsplit(['/', '\\']).next().unwrap_or(obj);
        if !rx!(r"^[0-9a-f]+-(.+)\.(?:o|obj)$")
            .captures(member)
            .is_some_and(|c| stems.contains(&c[1]))
        {
            return false;
        }
    }
    !crate::instructions::starts(name, &["_R", "_ZN", "_Z", "__"]) && !temp_label(name)
}
#[cfg(feature = "analyzer")]
pub fn shadowed_symbols(root: &Path) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for krate in std::fs::read_dir(root.join("crates"))? {
        let krate = krate?;
        if [
            "perry-runtime",
            "perry-stdlib",
            "perry-runtime-static",
            "perry-stdlib-static",
        ]
        .iter()
        .any(|s| krate.file_name() == *s)
        {
            continue;
        }
        let src = krate.path().join("src");
        if !src.is_dir() {
            continue;
        }
        for p in WalkDir::new(src) {
            let p = p?;
            if !p.file_type().is_file() || p.path().extension().is_none_or(|x| x != "rs") {
                continue;
            }
            let bytes = std::fs::read(p.path())?;
            let text = String::from_utf8_lossy(&bytes);
            for c in rx!(r#"extern\s+"C(?:-unwind)?"\s+fn\s+(\w+)"#).captures_iter(&text) {
                out.insert(c[1].to_owned());
            }
        }
    }
    Ok(out)
}
pub fn read_table(path: &Path) -> Result<Table> {
    parse_table(&std::fs::read_to_string(path)?, &path.display().to_string())
}
/// Parse the `symbol class throws` rows of a table; `#` lines are comments.
pub fn parse_table(text: &str, label: &str) -> Result<Table> {
    let mut out = Table::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() != 3 || !["no", "yes"].contains(&parts[2]) {
            bail!("{label}:{}: malformed table row {line:?}", n + 1);
        }
        let class = parts[1]
            .parse()
            .with_context(|| format!("{label}:{}", n + 1))?;
        if out
            .insert(
                parts[0].into(),
                Entry {
                    class,
                    throws: parts[2] == "yes",
                },
            )
            .is_some()
        {
            bail!("duplicate symbol {}", parts[0]);
        }
    }
    Ok(out)
}
#[derive(Debug, PartialEq, Eq)]
pub struct Drift {
    pub name: String,
    pub committed: String,
    pub fresh: String,
    pub throws: bool,
}
pub fn compare(committed: &Table, fresh: &Table) -> (Vec<Drift>, Vec<Drift>) {
    let names: BTreeSet<_> = committed.keys().chain(fresh.keys()).collect();
    let mut unsafe_drift = Vec::new();
    let mut safe = Vec::new();
    for throws in [false, true] {
        for name in &names {
            let c = committed.get(*name);
            let f = fresh.get(*name);
            let changed = if throws {
                c.map(|e| e.throws) != f.map(|e| e.throws)
            } else {
                c.map(|e| e.class) != f.map(|e| e.class)
            };
            let weaker = if throws {
                !c.is_none_or(|e| e.throws) && f.is_none_or(|e| e.throws)
            } else {
                c.map(|e| e.class).unwrap_or(Class::Reenters)
                    < f.map(|e| e.class).unwrap_or(Class::Reenters)
            };
            let text = |e: Option<&Entry>| {
                e.map(|e| {
                    if throws {
                        format!("throws={}", e.throws_name())
                    } else {
                        e.class.to_string()
                    }
                })
                .unwrap_or_else(|| {
                    if throws {
                        "throws=(absent)"
                    } else {
                        "(absent)"
                    }
                    .into()
                })
            };
            let row = Drift {
                name: (*name).clone(),
                committed: text(c),
                fresh: text(f),
                throws,
            };
            if weaker {
                unsafe_drift.push(row);
            } else if changed {
                safe.push(row);
            }
        }
    }
    (unsafe_drift, safe)
}
pub(crate) fn table_text(table: &Table, target: &str, archives: &[PathBuf]) -> String {
    let mut text = include_str!("table-header.txt")
        .replace("{target}", target)
        .replace(
            "{archives}",
            &archives
                .iter()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                .collect::<Vec<_>>()
                .join(" "),
        );
    for (name, row) in table {
        text.push_str(&format!("{name}\t{}\t{}\n", row.class, row.throws_name()));
    }
    text
}
pub fn write_table(table: &Table, target: &str, archives: &[PathBuf], out: &Path) -> Result<()> {
    let text = table_text(table, target, archives);
    let mut temp = out.as_os_str().to_owned();
    temp.push(".tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, out)?;
    Ok(())
}

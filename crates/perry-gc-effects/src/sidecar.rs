//! Sidecar files: one `<runtime archive>.gc-effects` per prebuilt runtime archive.
//!
//! The file holds the effects table of each link combination perry can pick for
//! that archive (the runtime alone, the runtime with its stdlib), keyed by the
//! size and SHA-256 of every archive in the combination. The archive file names
//! in the directory are the only input: no variant list exists anywhere.
#[cfg(feature = "analyzer")]
use crate::{analyze, Classification, Rule};
use crate::{parse_table, table::table_text, Table};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
#[cfg(feature = "analyzer")]
use std::time::Instant;

/// First line of every sidecar. A different version reads as a missing sidecar.
pub const SIDECAR_MAGIC: &str = "# perry-gc-effects sidecar v1";
/// File name extension appended to the runtime archive file name.
pub const SIDECAR_SUFFIX: &str = ".gc-effects";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveKey {
    pub name: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Combination {
    pub name: String,
    pub archives: Vec<ArchiveKey>,
    pub table: Table,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub analyzer: String,
    pub target: String,
    pub combinations: Vec<Combination>,
}

/// The sidecar path for a runtime archive: the archive file name plus `.gc-effects`.
pub fn sidecar_path(archive: &Path) -> PathBuf {
    let mut p = archive.as_os_str().to_owned();
    p.push(SIDECAR_SUFFIX);
    PathBuf::from(p)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
/// SHA-256 and size of the archive bytes.
pub fn archive_key(path: &Path) -> Result<ArchiveKey> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(ArchiveKey {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        size: bytes.len() as u64,
        sha256: hex(&Sha256::digest(&bytes)),
    })
}

/// The combination name for a link set: the runtime alone, or the runtime
/// with its stdlib.
pub fn combination_name(with_stdlib: bool) -> &'static str {
    if with_stdlib {
        "runtime+stdlib"
    } else {
        "runtime"
    }
}

/// Why a sidecar cannot speak for the archives about to be linked.
#[derive(Debug)]
pub enum Mismatch {
    /// The sidecar has no section for this combination.
    NoCombination(String),
    /// The section keys a different number of archives than the link set has.
    ArchiveCount { expected: usize, found: usize },
    /// An archive's bytes differ from the bytes the table was computed from.
    Bytes {
        archive: PathBuf,
        sidecar: Box<ArchiveKey>,
        found: Box<ArchiveKey>,
    },
}
impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCombination(c) => write!(f, "it has no [combination {c}] section"),
            Self::ArchiveCount { expected, found } => write!(
                f,
                "its section keys {expected} archive(s), the link set has {found}"
            ),
            Self::Bytes {
                archive,
                sidecar,
                found,
            } => write!(
                f,
                "{} is {} bytes sha256:{}, the sidecar was computed from {} ({} bytes sha256:{})",
                archive.display(),
                found.size,
                found.sha256,
                sidecar.name,
                sidecar.size,
                sidecar.sha256
            ),
        }
    }
}

impl Sidecar {
    /// The table of `combination`, if every archive of the link set (runtime
    /// first) has exactly the bytes the table was computed from. `keys` are
    /// the [`archive_key`]s of the archives perry links, in the same order.
    /// Only size and SHA-256 bind: a staged copy may carry another file name
    /// (`libperry_runtime_ios.a`), and the bytes are the key.
    pub fn verified_table(
        &self,
        combination: &str,
        archives: &[(&Path, &ArchiveKey)],
    ) -> std::result::Result<&Table, Mismatch> {
        let Some(c) = self.combinations.iter().find(|c| c.name == combination) else {
            return Err(Mismatch::NoCombination(combination.to_owned()));
        };
        if c.archives.len() != archives.len() {
            return Err(Mismatch::ArchiveCount {
                expected: c.archives.len(),
                found: archives.len(),
            });
        }
        for (want, (path, got)) in c.archives.iter().zip(archives) {
            if want.size != got.size || want.sha256 != got.sha256 {
                return Err(Mismatch::Bytes {
                    archive: path.to_path_buf(),
                    sidecar: Box::new(want.clone()),
                    found: Box::new((*got).clone()),
                });
            }
        }
        Ok(&c.table)
    }
}

impl Sidecar {
    pub fn render(&self) -> String {
        let mut out = format!(
            "{SIDECAR_MAGIC}\n# analyzer {}\n# target {}\n",
            self.analyzer, self.target
        );
        for c in &self.combinations {
            out.push_str(&format!("[combination {}]\n", c.name));
            for a in &c.archives {
                out.push_str(&format!(
                    "archive {} {} sha256:{}\n",
                    a.name, a.size, a.sha256
                ));
            }
            // The analyzer's table text without its `# target`/`# archives`
            // lines: the key block above replaces them.
            for line in table_text(&c.table, &self.target, &[]).lines() {
                if !line.starts_with("# target:") && !line.starts_with("# archives:") {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        out
    }
    /// Parse a sidecar. Any format other than this version is an error, which
    /// the consumer treats as a missing sidecar.
    pub fn parse(text: &str, label: &str) -> Result<Self> {
        let mut lines = text.lines().peekable();
        if lines.next() != Some(SIDECAR_MAGIC) {
            bail!("{label}: not a {SIDECAR_MAGIC:?} file");
        }
        let mut analyzer = None;
        let mut target = None;
        while let Some(l) = lines.peek() {
            if let Some(v) = l.strip_prefix("# analyzer ") {
                analyzer = Some(v.to_owned());
            } else if let Some(v) = l.strip_prefix("# target ") {
                target = Some(v.to_owned());
            } else {
                break;
            }
            lines.next();
        }
        let (Some(analyzer), Some(target)) = (analyzer, target) else {
            bail!("{label}: missing analyzer/target key lines");
        };
        let mut combinations: Vec<(String, Vec<ArchiveKey>, String)> = Vec::new();
        for (n, l) in lines.enumerate() {
            if let Some(name) = l
                .strip_prefix("[combination ")
                .and_then(|r| r.strip_suffix(']'))
            {
                combinations.push((name.to_owned(), Vec::new(), String::new()));
                continue;
            }
            let Some(cur) = combinations.last_mut() else {
                bail!("{label}: line {} before the first [combination]", n + 5);
            };
            if let Some(rest) = l.strip_prefix("archive ") {
                let f: Vec<_> = rest.split(' ').collect();
                let (Some(hash), 3) = (f.get(2).and_then(|h| h.strip_prefix("sha256:")), f.len())
                else {
                    bail!("{label}: malformed archive line {l:?}");
                };
                cur.1.push(ArchiveKey {
                    name: f[0].to_owned(),
                    size: f[1]
                        .parse()
                        .with_context(|| format!("{label}: archive size in {l:?}"))?,
                    sha256: hash.to_owned(),
                });
            } else {
                cur.2.push_str(l);
                cur.2.push('\n');
            }
        }
        let combinations = combinations
            .into_iter()
            .map(|(name, archives, rows)| {
                Ok(Combination {
                    table: parse_table(&rows, &format!("{label} [{name}]"))?,
                    name,
                    archives,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            analyzer,
            target,
            combinations,
        })
    }
    pub fn read(path: &Path) -> Result<Self> {
        Self::parse(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
            &path.display().to_string(),
        )
    }
}

#[cfg(feature = "analyzer")]
/// One runtime archive of a directory and the stdlib archive with the same
/// variant suffix, if the directory has one.
#[derive(Debug, PartialEq, Eq)]
pub struct Variant {
    pub runtime: PathBuf,
    pub stdlib: Option<PathBuf>,
}
#[cfg(feature = "analyzer")]
/// `libperry_runtime<suffix>.a` / `perry_runtime<suffix>.lib`, where the suffix
/// is empty or starts with `_` (`_abort`, `_core`, `_ios_sim`, ...).
fn archive_parts(file: &str, krate: &str) -> Option<(String, String)> {
    let stem = file
        .strip_suffix(".a")
        .map(|s| (s, "a"))
        .or_else(|| file.strip_suffix(".lib").map(|s| (s, "lib")))?;
    let stem_name = stem.0.strip_prefix("lib").unwrap_or(stem.0);
    let suffix = stem_name.strip_prefix(krate)?;
    (suffix.is_empty() || suffix.starts_with('_'))
        .then(|| (suffix.to_owned(), stem.1.to_owned()))
        .filter(|_| {
            // `libperry_runtime_static.a`-style names never ship, but a suffix
            // must be word characters only.
            suffix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}
#[cfg(feature = "analyzer")]
/// Every runtime archive in `dir`, in file name order. The file names are the
/// whole input: a new variant needs no change here.
pub fn discover(dir: &Path) -> Result<Vec<Variant>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut out = Vec::new();
    for n in &names {
        let Some((suffix, ext)) = archive_parts(n, "perry_runtime") else {
            continue;
        };
        let stdlib = names
            .iter()
            .find(|m| archive_parts(m, "perry_stdlib") == Some((suffix.clone(), ext.clone())))
            .map(|m| dir.join(m));
        out.push(Variant {
            runtime: dir.join(n),
            stdlib,
        });
    }
    if out.is_empty() {
        bail!(
            "no runtime archive (libperry_runtime*.a, perry_runtime*.lib) in {}",
            dir.display()
        );
    }
    Ok(out)
}

#[cfg(feature = "analyzer")]
/// `<os>-<arch>` of the objects in the archive (the object format names the OS).
pub fn detect_target(archive: &Path) -> Result<String> {
    use object::Object;
    let bytes = std::fs::read(archive)?;
    let ar = object::read::archive::ArchiveFile::parse(bytes.as_slice())?;
    for m in ar.members() {
        let m = m?;
        let data = m.data(bytes.as_slice())?;
        let Ok(file) = object::File::parse(data) else {
            continue;
        };
        let os = match file.format() {
            object::BinaryFormat::Elf => "linux",
            object::BinaryFormat::MachO => "macos",
            object::BinaryFormat::Coff => "windows",
            f => bail!("unsupported object format {f:?}"),
        };
        let arch = match file.architecture() {
            object::Architecture::X86_64 => "x86_64",
            object::Architecture::Aarch64 => "aarch64",
            a => bail!("unsupported architecture {a:?}"),
        };
        return Ok(format!("{os}-{arch}"));
    }
    bail!("no object file in {}", archive.display())
}

#[cfg(feature = "analyzer")]
/// Analyze every combination of every runtime archive in `dir` and return the
/// sidecars to write. Rule failures and the symbol floor are hard errors: a
/// failure writes nothing. `report` sees each combination's classification.
pub fn produce(
    dir: &Path,
    rules: &[Rule],
    root: &Path,
    analyzer: &str,
    target: Option<&str>,
    min_symbols: usize,
    report: &dyn Fn(&str, &Classification, f64),
) -> Result<Vec<(PathBuf, Sidecar)>> {
    let variants = discover(dir)?;
    let target = match target {
        Some(t) => t.to_owned(),
        None => detect_target(&variants[0].runtime)?,
    };
    let mut keys: std::collections::HashMap<PathBuf, ArchiveKey> = Default::default();
    let mut key = |p: &Path| -> Result<ArchiveKey> {
        if let Some(k) = keys.get(p) {
            return Ok(k.clone());
        }
        let k = archive_key(p)?;
        keys.insert(p.to_owned(), k.clone());
        Ok(k)
    };
    let mut out = Vec::new();
    for v in variants {
        let rt = v
            .runtime
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut combos = vec![("runtime", vec![v.runtime.clone()])];
        if let Some(s) = &v.stdlib {
            combos.push(("runtime+stdlib", vec![v.runtime.clone(), s.clone()]));
        }
        let mut combinations = Vec::new();
        for (name, archives) in combos {
            // Keys first: they bind the table to the bytes analyzed below.
            let archive_keys = archives
                .iter()
                .map(|a| key(a))
                .collect::<Result<Vec<_>>>()?;
            let t = Instant::now();
            let (_, res) =
                analyze(&archives, rules, root).with_context(|| format!("{rt} [{name}]"))?;
            if res.table.len() < min_symbols {
                bail!(
                    "{rt} [{name}]: only {} exported symbols (< {min_symbols})",
                    res.table.len()
                );
            }
            report(&format!("{rt} [{name}]"), &res, t.elapsed().as_secs_f64());
            combinations.push(Combination {
                name: name.into(),
                archives: archive_keys,
                table: res.table,
            });
        }
        out.push((
            sidecar_path(&v.runtime),
            Sidecar {
                analyzer: analyzer.to_owned(),
                target: target.clone(),
                combinations,
            },
        ));
    }
    Ok(out)
}
#[cfg(feature = "analyzer")]
pub fn write_sidecars(sidecars: &[(PathBuf, Sidecar)]) -> Result<()> {
    for (path, s) in sidecars {
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        std::fs::write(&temp, s.render())?;
        std::fs::rename(&temp, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Class, Entry};

    fn sample() -> Sidecar {
        let mut t = Table::new();
        t.insert(
            "js_a".into(),
            Entry {
                class: Class::Leaf,
                throws: false,
            },
        );
        t.insert(
            "js_b".into(),
            Entry {
                class: Class::Reenters,
                throws: true,
            },
        );
        let mut t2 = t.clone();
        t2.insert(
            "js_c".into(),
            Entry {
                class: Class::AllocOnly,
                throws: false,
            },
        );
        let key = |n: &str, s: u64| ArchiveKey {
            name: n.into(),
            size: s,
            sha256: hex(&Sha256::digest(n.as_bytes())),
        };
        Sidecar {
            analyzer: "0.5.0 abc123".into(),
            target: "linux-x86_64".into(),
            combinations: vec![
                Combination {
                    name: "runtime".into(),
                    archives: vec![key("libperry_runtime.a", 10)],
                    table: t,
                },
                Combination {
                    name: "runtime+stdlib".into(),
                    archives: vec![key("libperry_runtime.a", 10), key("libperry_stdlib.a", 20)],
                    table: t2,
                },
            ],
        }
    }
    #[test]
    fn format_round_trips_and_has_no_target_or_archives_header() {
        let s = sample();
        let text = s.render();
        assert!(text.starts_with("# perry-gc-effects sidecar v1\n# analyzer 0.5.0 abc123\n# target linux-x86_64\n[combination runtime]\narchive libperry_runtime.a 10 sha256:"));
        assert_eq!(
            text.matches("# target").count(),
            1,
            "table header target line dropped"
        );
        assert!(!text.contains("# archives:"));
        assert!(text.contains("js_a\tLeaf\tno\njs_b\tReenters\tyes\n"));
        assert_eq!(Sidecar::parse(&text, "t").unwrap(), s);
        assert_eq!(Sidecar::parse(&text, "t").unwrap().render(), text);
    }
    #[test]
    fn other_versions_and_damage_do_not_parse() {
        let text = sample().render();
        assert!(Sidecar::parse(&text.replace("sidecar v1", "sidecar v2"), "t").is_err());
        assert!(Sidecar::parse("", "t").is_err());
        assert!(Sidecar::parse(&text.replace("sha256:", "md5:"), "t").is_err());
        assert!(Sidecar::parse(&text.replace("js_a\tLeaf\tno", "js_a\tLeaf"), "t").is_err());
        assert!(Sidecar::parse(&text.replace("# target linux-x86_64\n", ""), "t").is_err());
    }
    #[test]
    fn key_is_size_and_sha256_of_the_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("libperry_runtime.a");
        std::fs::write(&p, b"abc").unwrap();
        let k = archive_key(&p).unwrap();
        assert_eq!(k.name, "libperry_runtime.a");
        assert_eq!(k.size, 3);
        assert_eq!(
            k.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::write(&p, b"abd").unwrap();
        assert_ne!(archive_key(&p).unwrap().sha256, k.sha256);
        assert_eq!(
            sidecar_path(&p).file_name().unwrap(),
            "libperry_runtime.a.gc-effects"
        );
    }
    #[test]
    fn verified_table_binds_the_section_to_size_and_sha256() {
        let s = sample();
        let tmp = tempfile::tempdir().unwrap();
        let rt = tmp.path().join("libperry_runtime_ios.a");
        // `sample()` keys each archive by the hash of its name: write those
        // bytes so the real key matches, under a different file name.
        let write = |p: &Path, name: &str, size: u64| {
            std::fs::write(p, name.as_bytes()).unwrap();
            let mut k = archive_key(p).unwrap();
            k.size = size;
            k
        };
        let k_rt = write(&rt, "libperry_runtime.a", 10);
        let st = tmp.path().join("libperry_stdlib.a");
        let k_st = write(&st, "libperry_stdlib.a", 20);
        let t = s
            .verified_table("runtime", &[(rt.as_path(), &k_rt)])
            .unwrap();
        assert_eq!(t.len(), 2);
        let t = s
            .verified_table(
                "runtime+stdlib",
                &[(rt.as_path(), &k_rt), (st.as_path(), &k_st)],
            )
            .unwrap();
        assert_eq!(t.len(), 3);
        // One flipped byte, a different size, a missing section, a short list.
        let mut flipped = k_rt.clone();
        flipped.sha256.replace_range(0..1, "x");
        assert!(matches!(
            s.verified_table("runtime", &[(rt.as_path(), &flipped)]),
            Err(Mismatch::Bytes { .. })
        ));
        let mut resized = k_rt.clone();
        resized.size += 1;
        assert!(s
            .verified_table("runtime", &[(rt.as_path(), &resized)])
            .is_err());
        assert!(matches!(
            s.verified_table("runtime+instr", &[(rt.as_path(), &k_rt)]),
            Err(Mismatch::NoCombination(_))
        ));
        assert!(matches!(
            s.verified_table("runtime+stdlib", &[(rt.as_path(), &k_rt)]),
            Err(Mismatch::ArchiveCount { .. })
        ));
        // The order is the link order: runtime first.
        assert!(s
            .verified_table(
                "runtime+stdlib",
                &[(st.as_path(), &k_st), (rt.as_path(), &k_rt)],
            )
            .is_err());
        assert_eq!(combination_name(false), "runtime");
        assert_eq!(combination_name(true), "runtime+stdlib");
    }
    #[cfg(feature = "analyzer")]
    #[test]
    fn variants_come_from_file_names() {
        let tmp = tempfile::tempdir().unwrap();
        for n in [
            "libperry_runtime.a",
            "libperry_runtime_abort.a",
            "libperry_runtime_instr.a",
            "libperry_runtime_ios_sim.a",
            "libperry_stdlib.a",
            "libperry_stdlib_ios_sim.a",
            "libperry_runtime.rlib",
            "libperry_runtime.d",
            "libperry_runtimeextra.a",
            "libperry_ext_net.a",
            "libperry_runtime.a.gc-effects",
            "libperry_runtime.a.gc-effects.tmp",
        ] {
            std::fs::write(tmp.path().join(n), b"").unwrap();
        }
        let got: Vec<_> = discover(tmp.path())
            .unwrap()
            .into_iter()
            .map(|v| {
                (
                    v.runtime
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    v.stdlib
                        .map(|s| s.file_name().unwrap().to_string_lossy().into_owned()),
                )
            })
            .collect();
        let s = |x: &str| Some(x.to_owned());
        assert_eq!(
            got,
            vec![
                ("libperry_runtime.a".to_owned(), s("libperry_stdlib.a")),
                ("libperry_runtime_abort.a".to_owned(), None),
                ("libperry_runtime_instr.a".to_owned(), None),
                (
                    "libperry_runtime_ios_sim.a".to_owned(),
                    s("libperry_stdlib_ios_sim.a")
                ),
            ]
        );
        let win = tempfile::tempdir().unwrap();
        for n in [
            "perry_runtime.lib",
            "perry_stdlib.lib",
            "perry_ui_windows.lib",
        ] {
            std::fs::write(win.path().join(n), b"").unwrap();
        }
        let v = discover(win.path()).unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].stdlib.is_some());
        assert!(discover(tmp.path().join("nope").as_path()).is_err());
    }
}

//! Shared-tokio archive coherence (#507 invariant, #7629 enforcement).
//!
//! # The invariant
//!
//! `perry-ext-{http,net,ws,fastify,…}` are `staticlib`s: each one *bundles* a
//! copy of every Rust crate it depends on, tokio included. perry-stdlib's
//! archive bundles tokio too, and perry-stdlib is the crate that owns the
//! process's one tokio runtime (`common::async_bridge`). A wrapper's async I/O
//! only works if **both archives bundle the same tokio compilation**, because
//! the runtime context every tokio entry point consults —
//! `tokio::runtime::context::CONTEXT` — is a `thread_local!` whose symbol is
//! mangled with the *compiling crate instance's* hash. Two tokio compilations
//! in one binary means two independent CONTEXT variables: perry-stdlib's
//! runtime enters one, and the wrapper reads the other, which is empty.
//!
//! The observable failure is a Rust panic on a worker thread — and because
//! shipping profiles are `panic = "abort"`, a SIGABRT (exit 134) rather than
//! an error the program could report:
//!
//! ```text
//! thread '<unnamed>' panicked at crates/perry-ext-http/src/server/server.rs:911:13:
//! there is no reactor running, must be called from the context of a Tokio 1.x runtime
//! ```
//!
//! It surfaces wherever the wrapper first needs the ambient runtime: at a
//! `tokio::spawn` call site in perry-ext-http's accept loop, or one frame
//! lower inside tokio itself (`net/tcp/listener.rs`'s `PollEvented::new` →
//! `Handle::current()`) for perry-ext-net's `TcpListener::bind`. Same cause,
//! same fix — the differing frame is only *where* the wrapper first touched
//! the reactor.
//!
//! # Why a check exists rather than just a fix
//!
//! The auto-optimize path already enforces the invariant by construction: it
//! rebuilds every tokio-using wrapper **in the same cargo invocation** as
//! perry-stdlib-static, so cargo unifies the dependency graph and both
//! archives get one tokio (`optimized_libs/driver.rs`, #507). Nothing checked
//! that it held, so every path that *bypasses* that rebuild — the
//! `PERRY_NO_AUTO_OPTIMIZE` route, the driver's "rebuild produced no archive"
//! fallback, a hand-run `cargo build -p perry-ext-http` — produced a binary
//! that linked cleanly and aborted at the first request, with the cause three
//! stages upstream of the symptom. #7629 sat open through two closes on
//! exactly that gap.
//!
//! This module makes the invariant *checkable*: the tokio compilation id is
//! readable straight out of an archive's member names (rustc names each
//! codegen unit `…tokio-<metadata-hash>.tokio.<cgu>…`), so the link path can
//! compare what it is about to link and refuse a pair it knows aborts.
//!
//! The check reports what it compared, not just a verdict — a
//! [`SharedTokioReport`] with an empty `checked` list is a check that did not
//! happen (no stdlib archive, or no tokio in it), and callers surface that
//! distinctly rather than treating it as a pass.

use std::collections::BTreeSet;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

/// One archive that was actually compared, and the tokio compilation it
/// bundles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckedArchive {
    /// File name as it will appear on the link line.
    pub(crate) name: String,
    /// `tokio-<hash>` as bundled by that archive.
    pub(crate) tokio_id: String,
    /// Whether it agrees with the stdlib archive.
    pub(crate) matches_stdlib: bool,
}

/// Outcome of [`verify_shared_tokio`]. `checked` is the live-subject
/// evidence: a report with nothing in it compared nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SharedTokioReport {
    /// The tokio compilation bundled by the stdlib archive, when there is one.
    pub(crate) stdlib_tokio_id: Option<String>,
    /// Every wrapper archive whose tokio was compared against it.
    pub(crate) checked: Vec<CheckedArchive>,
    /// Wrapper archives that must share tokio but bundle a different one.
    pub(crate) mismatched: Vec<CheckedArchive>,
}

impl SharedTokioReport {
    /// Did this run actually compare anything? A gate that reports success
    /// without this being true has measured nothing (CLAUDE.md's "a gate must
    /// assert its subject was live").
    pub(crate) fn compared_anything(&self) -> bool {
        !self.checked.is_empty()
    }
}

/// Read the member names of an `ar`-format archive (`.a` on Unix-likes,
/// `.lib` on Windows — same container).
///
/// Deliberately parses the container in-process instead of shelling out to
/// `llvm-ar t`, the way `strip_dedup` does: a coherence gate whose tool may be
/// absent is a gate that silently stops gating, which is the failure mode
/// CLAUDE.md's "four ways a gate can be unable to fail" list calls out. The
/// only thing this needs is the 60-byte member headers.
pub(crate) fn archive_member_names(path: &Path) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != b"!<arch>\n" {
        return Ok(Vec::new());
    }
    // The whole header chain is walked, but member *payloads* are skipped —
    // only the BSD long-name prefix and the GNU string table are ever read.
    let mut names = Vec::new();
    let mut gnu_strtab: Vec<u8> = Vec::new();
    loop {
        let mut header = [0u8; 60];
        match file.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        if &header[58..60] != b"`\n" {
            // Not a well-formed member header — stop rather than guess.
            break;
        }
        let raw_name = String::from_utf8_lossy(&header[0..16])
            .trim_end()
            .to_string();
        let size: u64 = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse()
            .unwrap_or(0);
        let mut consumed: u64 = 0;
        let name = if let Some(len_str) = raw_name.strip_prefix("#1/") {
            // BSD extended name (what Apple's `ar` and rustc emit on macOS):
            // the real name is the first `len` bytes of the payload.
            let len: usize = len_str.trim().parse().unwrap_or(0);
            let mut buf = vec![0u8; len.min(size as usize)];
            file.read_exact(&mut buf)?;
            consumed += buf.len() as u64;
            String::from_utf8_lossy(&buf)
                .trim_end_matches('\0')
                .to_string()
        } else if raw_name == "//" {
            // GNU long-name string table. Read it; its entries resolve the
            // `/<offset>` names below.
            gnu_strtab = vec![0u8; size as usize];
            file.read_exact(&mut gnu_strtab)?;
            consumed += size;
            String::new()
        } else if let Some(off_str) = raw_name.strip_prefix('/') {
            if let Ok(off) = off_str.trim().parse::<usize>() {
                gnu_strtab
                    .get(off..)
                    .map(|rest| {
                        let end = rest
                            .iter()
                            .position(|b| *b == b'/' || *b == b'\n')
                            .unwrap_or(rest.len());
                        String::from_utf8_lossy(&rest[..end]).to_string()
                    })
                    .unwrap_or_default()
            } else {
                // `/` alone is the symbol table — not a real member name.
                String::new()
            }
        } else {
            raw_name.trim_end_matches('/').to_string()
        };
        if !name.is_empty() {
            names.push(name);
        }
        // Skip the remaining payload, plus the even-alignment pad byte.
        let remaining = size.saturating_sub(consumed);
        let pad = size % 2;
        file.seek_relative((remaining + pad) as i64)?;
    }
    Ok(names)
}

/// Every distinct `tokio-<hash>` compilation an archive bundles.
///
/// rustc names each emitted codegen unit
/// `[<leaf>-<hash>.]<crate>-<metadata-hash>.<crate>.<cgu-hash>-cgu.N.rcgu.o`,
/// so the tokio compilation id is a dot-separated component. Matching the
/// component (rather than a substring) is what keeps `tokio_util-…` /
/// `tokio_rustls-…` / `tokio_tungstenite-…` from being mistaken for it.
pub(crate) fn tokio_compilation_ids(member_names: &[String]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for name in member_names {
        for component in name.split('.') {
            let Some(hash) = component.strip_prefix("tokio-") else {
                continue;
            };
            if !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                ids.insert(component.to_string());
            }
        }
    }
    ids
}

/// The one tokio compilation an archive bundles, if it bundles exactly one.
///
/// More than one means the archive is itself internally inconsistent (never
/// observed; cargo produces one compilation per feature-unified graph), and
/// `None` means the archive has no tokio at all — a CPU-only wrapper, or a
/// stdlib built without `async-runtime`.
fn archive_tokio_id(path: &Path) -> Option<String> {
    let names = archive_member_names(path).ok()?;
    let ids = tokio_compilation_ids(&names);
    if ids.len() == 1 {
        ids.into_iter().next()
    } else {
        None
    }
}

/// Library basenames (`perry_ext_http`, …) whose archive MUST bundle the same
/// tokio as perry-stdlib's.
///
/// Derived from the same predicate the auto-optimize rebuild uses to decide
/// which wrappers to fold into its cargo invocation, so the check and the fix
/// can never drift apart.
pub(crate) fn shared_tokio_lib_stems() -> BTreeSet<String> {
    super::well_known::iter_well_known()
        .filter(|b| {
            super::optimized_libs::binding_needs_shared_tokio(
                b.package.strip_prefix("node:").unwrap_or(&b.package),
            )
        })
        .map(|b| b.lib.clone())
        .collect()
}

/// Reduce a link-line path to the cargo `lib` name it carries:
/// `…/libperry_ext_http.a` and `…\perry_ext_http.lib` both give
/// `perry_ext_http`.
///
/// Splits on BOTH separators rather than going through `Path`, because
/// `Path::file_stem` only understands `\` on a Windows host — a cross-target
/// link line handled from a Unix host would otherwise reduce the whole
/// backslash path to one component and the check would silently skip the
/// archive.
fn archive_lib_stem(path: &Path) -> Option<&str> {
    let raw = path.to_str()?;
    let base = raw.rsplit(['/', '\\']).next()?;
    let stem = base
        .strip_suffix(".a")
        .or_else(|| base.strip_suffix(".lib"))
        .unwrap_or(base);
    Some(stem.strip_prefix("lib").unwrap_or(stem))
}

/// Does this link-line path name a wrapper archive bound by the invariant?
fn is_shared_tokio_archive(path: &Path, stems: &BTreeSet<String>) -> bool {
    archive_lib_stem(path).is_some_and(|stem| stems.contains(stem))
}

/// Compare the tokio compilation bundled by `stdlib_lib` against every
/// tokio-using wrapper archive on the link line.
///
/// Pure inspection — no side effects, no processes spawned — so it is safe to
/// run on every link.
pub(crate) fn verify_shared_tokio(
    stdlib_lib: Option<&Path>,
    well_known_libs: &[PathBuf],
) -> SharedTokioReport {
    let mut report = SharedTokioReport::default();
    let Some(stdlib_lib) = stdlib_lib else {
        return report;
    };
    let Some(stdlib_id) = archive_tokio_id(stdlib_lib) else {
        // No tokio in the stdlib archive: either it was built without
        // `async-runtime` (then no wrapper can reach a runtime through it and
        // the link would fail on `perry_ffi_spawn_*` first), or the archive is
        // unreadable. Either way there is nothing to compare, and the empty
        // `checked` list says so.
        return report;
    };
    let stems = shared_tokio_lib_stems();
    report.stdlib_tokio_id = Some(stdlib_id.clone());
    for lib in well_known_libs {
        if !is_shared_tokio_archive(lib, &stems) {
            continue;
        }
        let Some(id) = archive_tokio_id(lib) else {
            continue;
        };
        let entry = CheckedArchive {
            name: lib
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| lib.display().to_string()),
            tokio_id: id.clone(),
            matches_stdlib: id == stdlib_id,
        };
        if !entry.matches_stdlib {
            report.mismatched.push(entry.clone());
        }
        report.checked.push(entry);
    }
    report
}

/// Render the failure the way the reader needs it: what disagrees, why the
/// program would abort, and the one command that produces a coherent set.
///
/// The last part matters more than it looks. #7629's original FATAL-equivalent
/// (the tokio panic) names a source line in perry-ext-http, which is three
/// stages downstream of the mistake and sent every reader to the wrong crate.
pub(crate) fn mismatch_error_message(report: &SharedTokioReport, stdlib_lib: &Path) -> String {
    let stdlib_id = report.stdlib_tokio_id.as_deref().unwrap_or("<none>");
    let mut out = String::new();
    out.push_str(
        "error: the wrapper archive(s) below bundle a DIFFERENT tokio compilation than the \
         stdlib archive they would be linked with.\n",
    );
    out.push_str(&format!(
        "  {} bundles {}\n",
        stdlib_lib.display(),
        stdlib_id
    ));
    for m in &report.mismatched {
        out.push_str(&format!("  {} bundles {}\n", m.name, m.tokio_id));
    }
    out.push_str(
        "\nTwo tokio compilations in one binary means two independent \
         `tokio::runtime::context::CONTEXT` thread-locals. perry-stdlib's runtime enters one; \
         the wrapper reads the other, finds it empty, and the program aborts (SIGABRT, exit 134) \
         at its first socket or `tokio::spawn` with\n  \
         \"there is no reactor running, must be called from the context of a Tokio 1.x runtime\"\n\
         — see #507 and #7629. Linking this pair would produce that binary, so the link is \
         refused here instead.\n\n\
         fix: build the wrapper(s) in the SAME cargo invocation as the stdlib archive, e.g.\n  \
         cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static",
    );
    for m in &report.mismatched {
        if let Some(stem) = archive_lib_stem(Path::new(&m.name)) {
            out.push_str(&format!(" -p {}", stem.replace('_', "-")));
        }
    }
    out.push_str(
        "\n     (one invocation is what makes cargo unify tokio across them)\n  \
         or: unset PERRY_NO_AUTO_OPTIMIZE and let auto-optimize rebuild a coherent set itself.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tokio_id_is_read_from_a_dot_separated_component() {
        let members = names(&[
            "perry_stdlib-7655290ea30235cf.tokio-5aeb62139069856e.tokio.292ca503a36c1d82-cgu.0.rcgu.o.rcgu.o",
            "perry_stdlib-7655290ea30235cf.core-df38416008f914c9.core.318a34b566a36fe-cgu.0.rcgu.o.rcgu.o",
        ]);
        let ids = tokio_compilation_ids(&members);
        assert_eq!(
            ids.into_iter().collect::<Vec<_>>(),
            vec!["tokio-5aeb62139069856e".to_string()]
        );
    }

    #[test]
    fn sibling_tokio_crates_are_not_mistaken_for_tokio() {
        // These three all start with "tokio" and all appear next to the real
        // one in every archive. A substring match would report four distinct
        // "tokio" compilations and make the check useless.
        let members = names(&[
            "tokio_util-2e43c96694a42e07.tokio_util.aa67af5cd9fce282-cgu.0.rcgu.o",
            "tokio_rustls-844b0d2ba5508268.tokio_rustls.5b080bbd0f54d1f2-cgu.0.rcgu.o",
            "tokio_tungstenite-fe452cb16ad32fa1.tokio_tungstenite.6897a3b80a6baa8c-cgu.0.rcgu.o",
        ]);
        assert!(tokio_compilation_ids(&members).is_empty());

        let with_real = names(&[
            "tokio_util-2e43c96694a42e07.tokio_util.aa67af5cd9fce282-cgu.0.rcgu.o",
            "tokio-01c4c58f10c605f6.tokio.79ef538db9d49d8e-cgu.0.rcgu.o",
        ]);
        assert_eq!(
            tokio_compilation_ids(&with_real)
                .into_iter()
                .collect::<Vec<_>>(),
            vec!["tokio-01c4c58f10c605f6".to_string()]
        );
    }

    #[test]
    fn non_hex_suffix_is_not_a_compilation_id() {
        let members = names(&["tokio-notahash.tokio.deadbeef-cgu.0.rcgu.o"]);
        assert!(tokio_compilation_ids(&members).is_empty());
    }

    #[test]
    fn shared_tokio_stems_cover_the_wrappers_that_own_sockets() {
        let stems = shared_tokio_lib_stems();
        // The two archives #7629's witnesses abort in.
        assert!(stems.contains("perry_ext_http"), "{stems:?}");
        assert!(stems.contains("perry_ext_net"), "{stems:?}");
        assert!(stems.contains("perry_ext_ws"), "{stems:?}");
        // A CPU-only wrapper must NOT be in the set: it never enters a tokio
        // runtime context, so requiring a shared compilation would fail links
        // that work.
        assert!(!stems.contains("perry_ext_bcrypt"), "{stems:?}");
    }

    #[test]
    fn link_line_paths_are_matched_on_both_platform_spellings() {
        let stems = shared_tokio_lib_stems();
        assert!(is_shared_tokio_archive(
            Path::new("/x/target/release/libperry_ext_http.a"),
            &stems
        ));
        assert!(is_shared_tokio_archive(
            Path::new(r"C:\x\target\release\perry_ext_http.lib"),
            &stems
        ));
        assert!(!is_shared_tokio_archive(
            Path::new("/x/target/release/libperry_ext_bcrypt.a"),
            &stems
        ));
    }

    /// Round-trip through a real BSD-style archive (what macOS emits) so the
    /// container parser is exercised, not just the name matcher.
    #[test]
    fn bsd_long_names_are_read_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("libfake.a");
        let member = "perry_stdlib-7655290ea30235cf.tokio-5aeb62139069856e.tokio.292ca503a36c1d82-cgu.0.rcgu.o.rcgu.o";
        let payload = b"OBJECTBYTES";
        let name_bytes = member.as_bytes();
        let size = name_bytes.len() + payload.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"!<arch>\n");
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            format!("#1/{}", name_bytes.len()),
            0,
            0,
            0,
            "100644",
            size
        );
        assert_eq!(header.len(), 60);
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(payload);
        if size % 2 == 1 {
            bytes.push(b'\n');
        }
        std::fs::write(&path, &bytes).expect("write archive");

        let read_back = archive_member_names(&path).expect("read archive");
        assert_eq!(read_back, vec![member.to_string()]);
        assert_eq!(
            archive_tokio_id(&path).as_deref(),
            Some("tokio-5aeb62139069856e")
        );
    }

    #[test]
    fn a_report_that_compared_nothing_is_not_a_pass() {
        // No stdlib archive: nothing was compared, and `compared_anything`
        // must say so rather than the caller reading "no mismatches" as proof.
        let report = verify_shared_tokio(None, &[PathBuf::from("libperry_ext_http.a")]);
        assert!(!report.compared_anything());
        assert!(report.mismatched.is_empty());
        assert!(report.stdlib_tokio_id.is_none());
    }

    #[test]
    fn mismatch_message_names_both_ids_and_the_fixing_command() {
        let report = SharedTokioReport {
            stdlib_tokio_id: Some("tokio-5aeb62139069856e".to_string()),
            checked: vec![CheckedArchive {
                name: "libperry_ext_http.a".to_string(),
                tokio_id: "tokio-01c4c58f10c605f6".to_string(),
                matches_stdlib: false,
            }],
            mismatched: vec![CheckedArchive {
                name: "libperry_ext_http.a".to_string(),
                tokio_id: "tokio-01c4c58f10c605f6".to_string(),
                matches_stdlib: false,
            }],
        };
        let msg = mismatch_error_message(&report, Path::new("/x/libperry_stdlib.a"));
        assert!(msg.contains("tokio-5aeb62139069856e"), "{msg}");
        assert!(msg.contains("tokio-01c4c58f10c605f6"), "{msg}");
        assert!(msg.contains("there is no reactor running"), "{msg}");
        assert!(msg.contains("-p perry-ext-http"), "{msg}");
        assert!(msg.contains("perry-stdlib-static"), "{msg}");
    }
}

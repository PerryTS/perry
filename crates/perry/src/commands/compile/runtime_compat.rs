//! Compiler/runtime archive compatibility stamping (#8752).
//!
//! A stale `libperry_runtime` used to pass discovery and fail much later with
//! undefined symbols. The runtime now embeds a small version/build record;
//! this module streams the archive to find it and rejects skew before linking.

use anyhow::{bail, Result};
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

const STAMP_MAGIC: &str = "PERRY_RUNTIME_BUILD_STAMP_V1";
const STAMP_PREFIX: &[u8] = b"PERRY_RUNTIME_BUILD_STAMP_V1|";
const MAX_STAMP_LEN: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeBuildStamp {
    version: String,
    build_id: String,
}

impl RuntimeBuildStamp {
    fn current() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_id: perry_runtime::PERRY_RUNTIME_BUILD_ID.to_string(),
        }
    }

    fn parse(bytes: &[u8]) -> std::result::Result<Self, String> {
        let text =
            std::str::from_utf8(bytes).map_err(|error| format!("stamp is not UTF-8: {error}"))?;
        let mut fields = text.split('|');
        if fields.next() != Some(STAMP_MAGIC) {
            return Err("unexpected stamp format".to_string());
        }
        let version = fields
            .next()
            .and_then(|field| field.strip_prefix("version="))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "stamp has no version".to_string())?;
        let build_id = fields
            .next()
            .and_then(|field| field.strip_prefix("build="))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "stamp has no build id".to_string())?;
        if fields.next().is_some() {
            return Err("stamp has unexpected fields".to_string());
        }
        Ok(Self {
            version: version.to_string(),
            build_id: build_id.to_string(),
        })
    }

    fn short_build_id(&self) -> String {
        let (kind, value) = self
            .build_id
            .split_once(':')
            .unwrap_or(("build", self.build_id.as_str()));
        let short: String = value.chars().take(12).collect();
        match kind {
            "git" => format!("commit {short}"),
            "src" => format!("source {short}"),
            _ => format!("build {short}"),
        }
    }
}

impl fmt::Display for RuntimeBuildStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{} ({})", self.version, self.short_build_id())
    }
}

#[derive(Debug)]
pub(crate) enum RuntimeLibraryStatus {
    Compatible(RuntimeBuildStamp),
    MissingStamp,
    MalformedStamp(String),
    Mismatch {
        expected: RuntimeBuildStamp,
        found: RuntimeBuildStamp,
    },
    Unreadable(io::Error),
}

enum ScannedStamp {
    Missing,
    Bytes(Vec<u8>),
    Unterminated,
}

/// Stream instead of reading the whole archive into memory. Release runtime
/// archives can be tens of megabytes, while the record is at most 512 bytes.
fn scan_stamp(path: &Path) -> io::Result<ScannedStamp> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut buffer = [0_u8; 64 * 1024];
    let mut prefix_match = 0_usize;
    let mut record: Option<Vec<u8>> = None;

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(match record {
                Some(_) => ScannedStamp::Unterminated,
                None => ScannedStamp::Missing,
            });
        }
        for &byte in &buffer[..read] {
            if let Some(bytes) = record.as_mut() {
                if byte == 0 {
                    return Ok(ScannedStamp::Bytes(std::mem::take(bytes)));
                }
                if bytes.len() >= MAX_STAMP_LEN {
                    return Ok(ScannedStamp::Unterminated);
                }
                bytes.push(byte);
                continue;
            }

            if byte == STAMP_PREFIX[prefix_match] {
                prefix_match += 1;
                if prefix_match == STAMP_PREFIX.len() {
                    record = Some(STAMP_PREFIX.to_vec());
                    prefix_match = 0;
                }
            } else {
                // The marker has no multi-byte self-overlap; preserving a
                // leading `P` is enough to handle a mismatch at a new prefix.
                prefix_match = usize::from(byte == STAMP_PREFIX[0]);
            }
        }
    }
}

pub(crate) fn runtime_library_status(path: &Path) -> RuntimeLibraryStatus {
    let found = match scan_stamp(path) {
        Ok(ScannedStamp::Missing) => return RuntimeLibraryStatus::MissingStamp,
        Ok(ScannedStamp::Unterminated) => {
            return RuntimeLibraryStatus::MalformedStamp(
                "embedded stamp is unterminated or too long".to_string(),
            )
        }
        Ok(ScannedStamp::Bytes(bytes)) => match RuntimeBuildStamp::parse(&bytes) {
            Ok(stamp) => stamp,
            Err(error) => return RuntimeLibraryStatus::MalformedStamp(error),
        },
        Err(error) => return RuntimeLibraryStatus::Unreadable(error),
    };
    let expected = RuntimeBuildStamp::current();
    if found == expected {
        RuntimeLibraryStatus::Compatible(found)
    } else {
        RuntimeLibraryStatus::Mismatch { expected, found }
    }
}

pub(crate) fn runtime_library_diagnostic(path: &Path, status: &RuntimeLibraryStatus) -> String {
    let expected = RuntimeBuildStamp::current();
    let reason = match status {
        RuntimeLibraryStatus::Compatible(found) => {
            return format!("{} ({found}, matches this Perry)", path.display())
        }
        RuntimeLibraryStatus::MissingStamp => format!(
            "library build: unknown ({} has no build stamp and predates compatibility checks)\n  Perry build: {expected}",
            path.display()
        ),
        RuntimeLibraryStatus::MalformedStamp(error) => format!(
            "library build: unknown (invalid stamp in {}: {error})\n  Perry build: {expected}",
            path.display()
        ),
        RuntimeLibraryStatus::Mismatch { expected, found } => format!(
            "library build: {found}\n  Perry build: {expected}\n  library: {}",
            path.display()
        ),
        RuntimeLibraryStatus::Unreadable(error) => format!(
            "could not inspect {}: {error}\n  Perry build: {expected}",
            path.display()
        ),
    };

    format!(
        "runtime library does not match this Perry compiler:\n  {reason}\n\
         The archive may be stale. Rebuild it with \
         `cargo build --release -p perry-runtime-static`, then replace {}, \
         or reinstall Perry so the binary and libraries come from the same package.",
        path.display()
    )
}

pub(crate) fn ensure_runtime_library_compatible(path: &Path) -> Result<()> {
    let status = runtime_library_status(path);
    if matches!(&status, RuntimeLibraryStatus::Compatible(_)) {
        return Ok(());
    }
    bail!(runtime_library_diagnostic(path, &status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_archive(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("create archive fixture");
        file.write_all(bytes).expect("write archive fixture");
        file
    }

    fn encoded(stamp: &RuntimeBuildStamp) -> Vec<u8> {
        format!(
            "noise{STAMP_MAGIC}|version={}|build={}\0trailer",
            stamp.version, stamp.build_id
        )
        .into_bytes()
    }

    #[test]
    fn accepts_matching_embedded_stamp() {
        let expected = RuntimeBuildStamp::current();
        let archive = write_archive(&encoded(&expected));
        assert!(matches!(
            runtime_library_status(archive.path()),
            RuntimeLibraryStatus::Compatible(found) if found == expected
        ));
    }

    #[test]
    fn finds_stamp_across_reader_chunk_boundary() {
        let expected = RuntimeBuildStamp::current();
        let mut bytes = vec![b'x'; 64 * 1024 - 7];
        bytes.extend(encoded(&expected));
        let archive = write_archive(&bytes);
        assert!(matches!(
            runtime_library_status(archive.path()),
            RuntimeLibraryStatus::Compatible(_)
        ));
    }

    #[test]
    fn rejects_unstamped_legacy_archive_with_refresh_help() {
        let archive = write_archive(b"!<arch>\nlegacy runtime contents");
        let status = runtime_library_status(archive.path());
        assert!(matches!(&status, RuntimeLibraryStatus::MissingStamp));
        let diagnostic = runtime_library_diagnostic(archive.path(), &status);
        assert!(diagnostic.contains("has no build stamp"));
        assert!(diagnostic.contains("perry-runtime-static"));
        assert!(diagnostic.contains(&archive.path().display().to_string()));
    }

    #[test]
    fn rejects_mismatched_archive_and_names_both_builds() {
        let expected = RuntimeBuildStamp::current();
        let stale = RuntimeBuildStamp {
            version: "0.0.1".to_string(),
            build_id: "git:1111111111111111111111111111111111111111".to_string(),
        };
        let archive = write_archive(&encoded(&stale));
        let status = runtime_library_status(archive.path());
        assert!(matches!(&status, RuntimeLibraryStatus::Mismatch { .. }));
        let diagnostic = runtime_library_diagnostic(archive.path(), &status);
        assert!(diagnostic.contains(&stale.to_string()));
        assert!(diagnostic.contains(&expected.to_string()));
        assert!(diagnostic.contains("archive may be stale"));
    }

    #[test]
    fn rejects_unterminated_stamp() {
        let archive =
            write_archive(format!("{STAMP_MAGIC}|version=1.0.0|build=git:abc").as_bytes());
        assert!(matches!(
            runtime_library_status(archive.path()),
            RuntimeLibraryStatus::MalformedStamp(_)
        ));
    }
}

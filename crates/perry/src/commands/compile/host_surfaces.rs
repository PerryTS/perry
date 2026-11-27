//! Host surfaces (HOSTSURF §2.1): every `host-surface-v1` manifest the
//! build links, and the build inputs that add globals the static realm does
//! not declare. Whole-program facts read `%Global%` as complete, so such an
//! input with no surface declaring the globals is a compile error when the
//! facts run (the guard, §0.2): never an unknown global, never a run-time
//! check.
//!
//! Sources: `--host-surface PATH` (the target runtime's manifest, named by
//! the build), `host-surface-v1.json` beside the runtime library
//! (`PERRY_RUNTIME_DIR`), and a native library's package.json
//! `perry.hostSurface` (a path relative to the package). A native library
//! whose package declares `perry.hostGlobals: true` adds globals at realm
//! init and requires a surface that extends `perry-realm:%Global%`.
use anyhow::{bail, Context, Result};
use perry_hir::realm_manifest::host_surface as hs;
use std::path::{Path, PathBuf};

pub(crate) struct HostSurfaces {
    pub surfaces: Vec<hs::HostSurface>,
    pub requirements: Vec<hs::HostRequirement>,
}

fn read(path: &Path) -> Result<hs::HostSurface> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("reading host surface {}", path.display()))?;
    hs::parse(&json, &path.display().to_string()).map_err(anyhow::Error::msg)
}

pub(crate) fn collect(
    explicit: &[PathBuf],
    native_libraries: &[super::types::NativeLibraryManifest],
    target: Option<&str>,
) -> Result<HostSurfaces> {
    let target = target.unwrap_or("host");
    let mut surfaces = Vec::new();
    let mut requirements = Vec::new();
    // Named by the build: included whatever its `targets` list.
    for p in explicit {
        surfaces.push(read(p)?);
    }
    if let Some(dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        let p = Path::new(&dir).join(hs::HOST_SURFACE_FILE);
        if p.is_file() {
            let s = read(&p)?;
            if hs::for_target(&s, target) {
                surfaces.push(s);
            }
        }
    }
    for lib in native_libraries {
        let pkg = lib.package_dir.join("package.json");
        let Ok(text) = std::fs::read_to_string(&pkg) else {
            continue;
        };
        let v: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", pkg.display()))?;
        let perry = v.get("perry");
        match perry.and_then(|p| p.get("hostGlobals")) {
            None | Some(serde_json::Value::Bool(false)) => {}
            Some(serde_json::Value::Bool(true)) => requirements.push(hs::HostRequirement {
                by: format!("native library `{}`", lib.module),
            }),
            Some(other) => bail!(
                "{}: `perry.hostGlobals` must be true or false, found {other}",
                pkg.display()
            ),
        }
        match perry.and_then(|p| p.get("hostSurface")) {
            None => {}
            Some(serde_json::Value::String(rel)) => {
                let s = read(&lib.package_dir.join(rel))?;
                if hs::for_target(&s, target) {
                    surfaces.push(s);
                }
            }
            Some(other) => bail!(
                "{}: `perry.hostSurface` must be a path string, found {other}",
                pkg.display()
            ),
        }
    }
    Ok(HostSurfaces { surfaces, requirements })
}

/// The guard: refuses whole-program facts over a realm whose host adds
/// globals no loaded surface declares; then loads the surfaces into the
/// facts' host realm (HOSTSURF H2: the solver reads them beside the static
/// realm), a load error being a compile error. `None`: facts off, or no
/// surface.
pub(crate) fn guard(
    h: &HostSurfaces,
    facts_on: bool,
) -> Result<Option<&'static perry_wpfacts::host::HostRealm>> {
    if !facts_on {
        return Ok(None);
    }
    hs::check_required(&h.requirements, &h.surfaces).map_err(anyhow::Error::msg)?;
    perry_wpfacts::host::load(&h.surfaces).map_err(anyhow::Error::msg)
}

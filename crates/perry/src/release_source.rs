//! Where Perry asks "what is the latest version?".
//!
//! Until now there was one answer: walk a fixed list of URLs — an override, the
//! config, Perry Hub, then the GitHub releases API — and read a
//! GitHub-releases-shaped document from whichever replied first. That is fine
//! when everyone installs the same way, and wrong as soon as they do not: an
//! npm user's "latest" is whatever the registry's `latest` dist-tag says, and
//! asking GitHub instead can announce a version their package manager cannot
//! yet install.
//!
//! So the source is now a choice, with four shapes.
//!
//! # The split that matters: checking is not downloading
//!
//! A check source answers one question and returns [`VersionProbe`]. It does
//! **not** decide where the binary comes from. Artifacts and their signed
//! manifest always resolve from the release infrastructure, through
//! [`release_info_servers`], whatever the check source is.
//!
//! That separation is deliberate and load-bearing. The manifest is what makes
//! a self-update trustworthy — Ed25519 over the artifact's digest and version —
//! and a check source is a URL a user can point anywhere. Letting the check
//! source redirect the download would turn a configuration setting into a way
//! to install an arbitrary binary. Whoever answers "what is new?" never gets to
//! answer "what should I run?".
//!
//! # Why the npm shapes send no credentials to a public registry
//!
//! The public registry needs no auth, and the abbreviated packument
//! (`Accept: application/vnd.npm.install-v1+json`) is the cheap, cacheable
//! document intended for exactly this question. GitHub Packages does need a
//! token, so that shape reads `GH_TOKEN` / `GITHUB_TOKEN` — the same variables
//! `gh` and every CI job already set — and simply fails when neither is
//! present rather than retrying without them.

use anyhow::{Context, Result};
use serde::Deserialize;

/// The public npm registry, used when a source names a package but no registry.
const NPM_REGISTRY: &str = "https://registry.npmjs.org";
/// GitHub Packages' npm endpoint.
const GH_REGISTRY: &str = "https://npm.pkg.github.com";
/// The npm package Perry publishes its wrapper as.
const PERRY_NPM_PACKAGE: &str = "@perryts/perry";

/// What every source returns, whatever document it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VersionProbe {
    pub(crate) latest_version: String,
    /// Somewhere a human can read about this version. Never used to download.
    pub(crate) release_url: String,
    /// RFC3339 publish time, when the source reports one. Feeds the release
    /// cooldown: a version too fresh to have been noticed by anyone yet is not
    /// one to install unattended.
    pub(crate) published_at: Option<String>,
    /// A one-line title, when the source has one.
    pub(crate) headline: Option<String>,
}

/// Which document to read, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckSource {
    /// A GitHub releases API URL. The historical behaviour, and the only shape
    /// that also carries the assets the installer needs.
    GhReleases { url: String },
    /// An npm registry packument — the public registry unless told otherwise.
    Npm { package: String, registry: String },
    /// GitHub Packages, which is npm-shaped but always authenticated.
    GhRegistry { package: String, registry: String },
    /// Any HTTPS URL returning `{ "version": ..., "release_url": ... }`.
    Custom { url: String },
}

/// Parse a configured `source` name into a source, given the other keys.
///
<<<<<<< HEAD
/// Returns `None` for a name this build does not know, so the caller can fall
/// back rather than fail — a config written by a newer Perry must not break an
/// older one's update check.
=======
/// One spelling per source, deliberately: a set of accepted aliases is a
/// surface to keep documented and tested forever in exchange for saving one
/// lookup in the docs.
///
/// Returns `None` for a name this build does not know, so the caller falls back
/// to the default rather than refusing to check at all. That is not a
/// compatibility affordance — it is that an update check is the wrong place to
/// turn a config typo into a hard failure.
>>>>>>> 373f3fdd1 (feat(cli): hold back brand-new releases, remember a skipped version, and document the whole surface)
pub(crate) fn from_config(
    source: Option<&str>,
    package: Option<&str>,
    registry: Option<&str>,
    server: Option<&str>,
) -> Option<CheckSource> {
    let package = || package.unwrap_or(PERRY_NPM_PACKAGE).to_string();
    match source?.trim().to_ascii_lowercase().as_str() {
<<<<<<< HEAD
        "gh-releases" | "github" => Some(CheckSource::GhReleases {
=======
        "gh-releases" => Some(CheckSource::GhReleases {
>>>>>>> 373f3fdd1 (feat(cli): hold back brand-new releases, remember a skipped version, and document the whole surface)
            url: server
                .unwrap_or(super::update_checker::GITHUB_URL)
                .to_string(),
        }),
<<<<<<< HEAD
        "npm" | "npm-registry" => Some(CheckSource::Npm {
            package: package(),
            registry: registry.unwrap_or(NPM_REGISTRY).to_string(),
        }),
        "gh-registry" | "github-packages" => Some(CheckSource::GhRegistry {
=======
        "npm" => Some(CheckSource::Npm {
            package: package(),
            registry: registry.unwrap_or(NPM_REGISTRY).to_string(),
        }),
        "gh-registry" => Some(CheckSource::GhRegistry {
>>>>>>> 373f3fdd1 (feat(cli): hold back brand-new releases, remember a skipped version, and document the whole surface)
            package: package(),
            registry: registry.unwrap_or(GH_REGISTRY).to_string(),
        }),
        // `custom` without a URL is meaningless rather than harmless: silently
        // treating it as "the default ladder" would hide the missing key.
        "custom" => server.map(|url| CheckSource::Custom {
            url: url.to_string(),
        }),
        _ => None,
    }
}

/// The source to use when the config names none.
///
/// An npm-managed install asks npm, because that is the version its own
/// package manager can actually install — announcing a GitHub release the
/// wrapper package has not published yet is worse than saying nothing. Every
/// other install falls back to the historical ladder, which is what
/// [`release_info_servers`] walks.
pub(crate) fn default_for_channel(
    channel: crate::install_channel::InstallChannel,
) -> Option<CheckSource> {
    match channel {
        crate::install_channel::InstallChannel::Npm => Some(CheckSource::Npm {
            package: PERRY_NPM_PACKAGE.to_string(),
            registry: NPM_REGISTRY.to_string(),
        }),
        _ => None,
    }
}

/// The release-infrastructure URLs, in preference order.
///
/// This is the ARTIFACT ladder as well as the fallback check ladder, and it is
/// the only thing the installer reads: it is where the signed `.update.json`
/// manifest lives. `PERRY_UPDATE_SERVER` and `[update] server` still come
/// first, which is what makes a private mirror work.
pub(crate) fn release_info_servers() -> Vec<String> {
    let mut servers = Vec::new();
    // A configured server is dropped unless it is HTTPS or loopback. Plain HTTP
    // here is not only readable in transit — anyone who can answer the request
    // can suppress updates indefinitely by reporting the running version as the
    // latest one, which is a silent way to keep a machine on a vulnerable build.
    if let Ok(url) = std::env::var("PERRY_UPDATE_SERVER") {
        if !url.is_empty() && url_is_secure(&url) {
            servers.push(url);
        }
    }
    if servers.is_empty() {
        if let Some(url) = crate::commands::publish::load_config()
            .update
            .and_then(|u| u.server)
        {
            if url_is_secure(&url) {
                servers.push(url);
            }
        }
    }
    servers.push(super::update_checker::HUB_URL.to_string());
    servers.push(super::update_checker::GITHUB_URL.to_string());
    servers
}

/// Is this URL safe to send a version check to?
///
/// HTTPS, or loopback so a local test server still works. Loopback needs a `:`,
/// a `/`, or the end of the string after the prefix, or `http://localhost.evil`
/// would pass as localhost.
pub(crate) fn url_is_secure(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| {
            lower.strip_prefix(prefix).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(':') || rest.starts_with('/')
            })
        })
}

/// The source this run should use: the config's choice, else the install
/// channel's default, else none — meaning "walk the historical ladder".
pub(crate) fn resolve() -> Option<CheckSource> {
    let config = crate::commands::publish::load_config()
        .update
        .unwrap_or_default();
    from_config(
        config.source.as_deref(),
        config.package.as_deref(),
        config.registry.as_deref(),
        config.server.as_deref(),
    )
    .or_else(|| default_for_channel(crate::install_channel::detect()))
}

impl CheckSource {
    /// The URL to request, and the headers this shape needs.
    /// Reject anything that is not an absolute HTTPS URL without credentials.
    ///
    /// The artifact path already required this; the CHECK path did not, and it is
    /// the more dangerous of the two for `gh-registry`: an `http://` registry
    /// would have had `Authorization: Bearer <token>` attached to a plaintext
    /// request, putting a GitHub token on the wire. Loopback is exempt so a local
    /// test server still works.
    fn require_secure(label: &str, url: &str) -> Result<()> {
        let lower = url.to_ascii_lowercase();
        if !url_is_secure(url) {
            anyhow::bail!(
                "[update] {label} must be an https:// URL (got `{url}`). \
                 Loopback http:// is allowed for local testing."
            );
        }
        // Credentials in a URL are sent to whatever host follows the `@`, and
        // land in logs besides.
        let authority = lower
            .split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
            .unwrap_or_default();
        if authority.contains('@') {
            anyhow::bail!("[update] {label} must not embed credentials: `{url}`");
        }
        Ok(())
    }

    pub(crate) fn request(&self) -> Result<(String, Vec<(&'static str, String)>)> {
        match self {
            Self::GhReleases { url } | Self::Custom { url } => {
                Self::require_secure("server", url)?;
                Ok((url.clone(), Vec::new()))
            }
            Self::Npm { package, registry } => Ok((
                {
                    Self::require_secure("registry", registry)?;
                    packument_url(registry, package)
                },
                // The abbreviated document: smaller, cacheable, and the one npm
                // itself asks for. No credentials — the public registry wants
                // none, and sending a token to it would be a leak, not a
                // convenience.
                vec![("Accept", "application/vnd.npm.install-v1+json".to_string())],
            )),
            Self::GhRegistry { package, registry } => {
                let token = std::env::var("GH_TOKEN")
                    .or_else(|_| std::env::var("GITHUB_TOKEN"))
                    .ok()
                    .filter(|t| !t.is_empty())
                    .context(
                        "GitHub Packages needs a token: set GH_TOKEN or GITHUB_TOKEN, \
                         or use `source = \"npm\"` for the public registry",
                    )?;
                // Checked BEFORE the token is attached, not after.
                Self::require_secure("registry", registry)?;
                Ok((
                    packument_url(registry, package),
                    vec![
                        ("Accept", "application/vnd.npm.install-v1+json".to_string()),
                        ("Authorization", format!("Bearer {token}")),
                    ],
                ))
            }
        }
    }

    /// Turn this shape's response body into a probe.
    pub(crate) fn parse(&self, body: &str) -> Result<VersionProbe> {
        match self {
            Self::GhReleases { .. } => parse_gh_release(body),
            Self::Npm { package, .. } | Self::GhRegistry { package, .. } => {
                parse_packument(body, package)
            }
            Self::Custom { .. } => parse_custom(body),
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::GhReleases { .. } => "gh-releases",
            Self::Npm { .. } => "npm",
            Self::GhRegistry { .. } => "gh-registry",
            Self::Custom { .. } => "custom",
        }
    }
}

/// npm requires the scope's `/` to be percent-encoded in a packument path.
fn packument_url(registry: &str, package: &str) -> String {
    format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        package.replace('/', "%2F")
    )
}

fn parse_gh_release(body: &str) -> Result<VersionProbe> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        published_at: Option<String>,
    }
    let release: Release = serde_json::from_str(body)
        .context("update server returned a document that is not a release")?;
    Ok(VersionProbe {
        latest_version: release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name)
            .to_string(),
        release_url: release.html_url,
        published_at: release.published_at,
        headline: release.name.filter(|n| !n.trim().is_empty()),
    })
}

fn parse_packument(body: &str, package: &str) -> Result<VersionProbe> {
    #[derive(Deserialize)]
    struct Packument {
        #[serde(rename = "dist-tags")]
        dist_tags: DistTags,
        /// Present in the FULL packument, absent from the abbreviated one, so
        /// the cooldown falls back to "unknown" rather than to a wrong answer.
        #[serde(default)]
        time: std::collections::HashMap<String, String>,
    }
    #[derive(Deserialize)]
    struct DistTags {
        latest: String,
    }
    let packument: Packument = serde_json::from_str(body)
        .context("registry returned a document without a `dist-tags.latest`")?;
    let latest = packument.dist_tags.latest;
    let published_at = packument.time.get(&latest).cloned();
    Ok(VersionProbe {
        release_url: format!("https://www.npmjs.com/package/{package}/v/{latest}"),
        latest_version: latest,
        published_at,
        headline: None,
    })
}

fn parse_custom(body: &str) -> Result<VersionProbe> {
    #[derive(Deserialize)]
    struct Manifest {
        version: String,
        #[serde(default)]
        release_url: Option<String>,
        #[serde(default)]
        published_at: Option<String>,
        #[serde(default)]
        notes: Option<String>,
    }
    let manifest: Manifest = serde_json::from_str(body)
        .context("a custom update source must return {\"version\": \"...\"}")?;
    Ok(VersionProbe {
        release_url: manifest.release_url.unwrap_or_default(),
        latest_version: manifest
            .version
            .strip_prefix('v')
            .unwrap_or(&manifest.version)
            .to_string(),
        published_at: manifest.published_at,
        headline: manifest.notes.filter(|n| !n.trim().is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_channel::InstallChannel;

    #[test]
    fn a_github_release_document_yields_a_probe() {
        let probe = parse_gh_release(
            r#"{
                "tag_name": "v0.5.1447",
                "html_url": "https://github.com/PerryTS/perry/releases/tag/v0.5.1447",
                "name": "Faster incremental builds",
                "published_at": "2026-08-10T09:00:00Z"
            }"#,
        )
        .expect("parse");
        assert_eq!(
            probe.latest_version, "0.5.1447",
            "the `v` prefix is stripped"
        );
        assert_eq!(probe.published_at.as_deref(), Some("2026-08-10T09:00:00Z"));
        assert_eq!(probe.headline.as_deref(), Some("Faster incremental builds"));
    }

    /// The abbreviated packument is what npm itself asks for and has no `time`
    /// map, so the cooldown must read "unknown" rather than inventing a date.
    #[test]
    fn an_abbreviated_packument_yields_a_version_without_a_date() {
        let probe = parse_packument(
            r#"{"dist-tags":{"latest":"0.5.1447"},"versions":{}}"#,
            "@perryts/perry",
        )
        .expect("parse");
        assert_eq!(probe.latest_version, "0.5.1447");
        assert_eq!(probe.published_at, None);
        assert!(probe.release_url.contains("@perryts/perry"));
    }

    #[test]
    fn a_full_packument_supplies_the_publish_time() {
        let probe = parse_packument(
            r#"{
                "dist-tags": {"latest": "0.5.1447"},
                "time": {"0.5.1446": "2026-08-01T00:00:00Z", "0.5.1447": "2026-08-10T09:00:00Z"}
            }"#,
            "@perryts/perry",
        )
        .expect("parse");
        assert_eq!(probe.published_at.as_deref(), Some("2026-08-10T09:00:00Z"));
    }

    #[test]
    fn a_custom_manifest_needs_only_a_version() {
        let probe = parse_custom(r#"{"version":"1.2.3"}"#).expect("parse");
        assert_eq!(probe.latest_version, "1.2.3");
        assert_eq!(probe.release_url, "");

        let full = parse_custom(
            r#"{"version":"v1.2.3","release_url":"https://example.test/1.2.3",
                 "published_at":"2026-08-10T09:00:00Z","notes":"Bug fixes"}"#,
        )
        .expect("parse");
        assert_eq!(full.latest_version, "1.2.3");
        assert_eq!(full.headline.as_deref(), Some("Bug fixes"));
    }

    /// Each shape must reject the others' documents rather than reading a
    /// field that happens to be there. A registry answering a gh-releases
    /// request must be an error, not a version of `""`.
    #[test]
    fn a_source_rejects_another_shapes_document() {
        assert!(parse_gh_release(r#"{"dist-tags":{"latest":"1.0.0"}}"#).is_err());
        assert!(parse_packument(r#"{"tag_name":"v1.0.0","html_url":"x"}"#, "p").is_err());
        assert!(parse_custom(r#"{"tag_name":"v1.0.0"}"#).is_err());
        assert!(parse_gh_release("not json at all").is_err());
    }

    /// A scoped package's `/` has to be percent-encoded, or the registry reads
    /// the scope as a path segment and answers 404.
    #[test]
    fn a_scoped_package_is_percent_encoded_in_the_path() {
        assert_eq!(
            packument_url("https://registry.npmjs.org/", "@perryts/perry"),
            "https://registry.npmjs.org/@perryts%2Fperry"
        );
    }

    #[test]
    fn config_names_map_to_sources_and_unknown_names_fall_back() {
        assert!(matches!(
            from_config(Some("npm"), None, None, None),
            Some(CheckSource::Npm { .. })
        ));
        assert!(matches!(
            from_config(Some("gh-registry"), None, None, None),
            Some(CheckSource::GhRegistry { .. })
        ));
        assert!(matches!(
            from_config(Some("gh-releases"), None, None, None),
            Some(CheckSource::GhReleases { .. })
        ));
        assert!(matches!(
            from_config(Some("custom"), None, None, Some("https://example.test/v")),
            Some(CheckSource::Custom { .. })
        ));
        // Unknown to THIS build — fall back rather than fail, so a config
        // written by a newer Perry does not break an older one.
        assert_eq!(from_config(Some("carrier-pigeon"), None, None, None), None);
        assert_eq!(from_config(None, None, None, None), None);
        // `custom` with no URL is a missing key, not a default.
        assert_eq!(from_config(Some("custom"), None, None, None), None);
    }

    /// An npm-managed install asks npm, because that is the version its own
    /// package manager can install. Announcing a GitHub release the wrapper
    /// package has not published yet is worse than saying nothing.
    #[test]
    fn an_npm_install_defaults_to_the_npm_registry() {
        assert!(matches!(
            default_for_channel(InstallChannel::Npm),
            Some(CheckSource::Npm { .. })
        ));
        for channel in [
            InstallChannel::SelfManaged,
            InstallChannel::Homebrew,
            InstallChannel::Apt,
            InstallChannel::Winget,
        ] {
            assert_eq!(
                default_for_channel(channel),
                None,
                "{} keeps the historical ladder",
                channel.label()
            );
        }
    }

    /// ★ The separation that keeps a config setting from becoming a way to
    /// install an arbitrary binary: no check source may name where the
    /// artifact comes from. The installer reads `release_info_servers` only.
    #[test]
    fn no_check_source_can_redirect_the_artifact_download() {
        let custom = CheckSource::Custom {
            url: "https://attacker.test/version.json".to_string(),
        };
        let (url, _) = custom.request().expect("request");
        assert_eq!(url, "https://attacker.test/version.json");

        // The artifact ladder is built from the release infrastructure and the
        // operator's own override, and knows nothing about the source above.
        let servers = release_info_servers();
        assert!(
            !servers.iter().any(|s| s.contains("attacker.test")),
            "a check source leaked into the artifact ladder: {servers:?}"
        );
        assert!(
            servers
                .iter()
                .any(|s| s == crate::update_checker::GITHUB_URL),
            "the release infrastructure must stay in the ladder: {servers:?}"
        );
    }

    /// A plaintext URL is refused BEFORE the token goes on the request.
    ///
    /// The check path is the dangerous one for `gh-registry`: an `http://`
    /// registry would have carried `Authorization: Bearer <token>` in clear
    /// text. Checking after building the headers would still leak on the retry.
    #[test]
    fn a_plaintext_registry_is_refused_before_a_token_is_attached() {
        let _lock = crate::test_env_lock::env_lock();
        let saved = std::env::var("GH_TOKEN").ok();
        std::env::set_var("GH_TOKEN", "super-secret");

        let insecure = CheckSource::GhRegistry {
            package: "@perryts/perry".to_string(),
            registry: "http://npm.internal.test".to_string(),
        };
        let error = insecure.request().expect_err("plaintext must be refused");
        let text = format!("{error:#}");
        assert!(text.contains("https://"), "{text}");
        assert!(
            !text.contains("super-secret"),
            "the error must not echo the token: {text}"
        );

        // Every other shape is checked too, not just the authenticated one.
        for source in [
            CheckSource::Custom {
                url: "http://updates.test/v".into(),
            },
            CheckSource::GhReleases {
                url: "http://api.test/latest".into(),
            },
            CheckSource::Npm {
                package: "p".into(),
                registry: "http://registry.test".into(),
            },
        ] {
            assert!(
                source.request().is_err(),
                "{} accepted a plaintext URL",
                source.label()
            );
        }

        // Loopback still works, so a local test server is usable.
        let local = CheckSource::Custom {
            url: "http://127.0.0.1:8080/v".into(),
        };
        assert!(local.request().is_ok());

        // And credentials in the URL are refused: they would be sent to whatever
        // host follows the `@`, and would land in logs besides.
        let creds = CheckSource::Custom {
            url: "https://user:pw@evil.test/v".into(),
        };
        assert!(
            creds.request().is_err(),
            "embedded credentials must be refused"
        );

        match saved {
            Some(v) => std::env::set_var("GH_TOKEN", v),
            None => std::env::remove_var("GH_TOKEN"),
        }
    }

    /// GitHub Packages always needs a token; failing with a sentence beats
    /// retrying unauthenticated and reporting a 404 as "up to date".
    #[test]
    fn gh_registry_without_a_token_fails_with_an_explanation() {
        let _lock = crate::test_env_lock::env_lock();
        let saved = (
            std::env::var("GH_TOKEN").ok(),
            std::env::var("GITHUB_TOKEN").ok(),
        );
        std::env::remove_var("GH_TOKEN");
        std::env::remove_var("GITHUB_TOKEN");

        let source = CheckSource::GhRegistry {
            package: "@perryts/perry".to_string(),
            registry: GH_REGISTRY.to_string(),
        };
        let error = source.request().expect_err("no token must be an error");
        let text = format!("{error:#}");
        assert!(
            text.contains("GH_TOKEN"),
            "the error must name the fix: {text}"
        );

        if let Some(v) = saved.0 {
            std::env::set_var("GH_TOKEN", v);
        }
        if let Some(v) = saved.1 {
            std::env::set_var("GITHUB_TOKEN", v);
        }
    }

    /// And the public registry must never be sent one.
    #[test]
    fn the_public_registry_is_asked_without_credentials() {
        let source = CheckSource::Npm {
            package: "@perryts/perry".to_string(),
            registry: NPM_REGISTRY.to_string(),
        };
        let (_, headers) = source.request().expect("request");
        assert!(
            !headers.iter().any(|(name, _)| *name == "Authorization"),
            "a token was sent to the public registry: {headers:?}"
        );
        assert!(headers
            .iter()
            .any(|(name, value)| *name == "Accept" && value.contains("install-v1")));
    }
}

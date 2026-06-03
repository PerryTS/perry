//! YAML parsing, environment variable interpolation, `.env` loading,
//! and multi-file merge.

use crate::error::{ComposeError, Result};
use crate::types::ComposeSpec;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ============ Environment variable interpolation ============

/// Expand `${VAR}`, `${VAR:-default}`, `${VAR:+value}`, and `$VAR` in a YAML string.
///
/// This is the primary public API for interpolation (spec name: `interpolate_yaml`).
pub fn interpolate_yaml(yaml: &str, env: &HashMap<String, String>) -> String {
    interpolate(yaml, env)
}

/// Internal interpolation engine — also exported for use in tests and other modules.
pub fn interpolate(input: &str, env: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            match chars.peek() {
                Some('{') => {
                    chars.next(); // consume '{'
                    let expr = read_until_close(&mut chars);
                    let expanded = expand_expr(&expr, env);
                    result.push_str(&expanded);
                }
                Some('$') => {
                    // $$ → literal $
                    chars.next();
                    result.push('$');
                }
                Some(&c) if c.is_alphanumeric() || c == '_' => {
                    let name = read_plain_var(&mut chars, c);
                    let val = lookup(&name, env);
                    result.push_str(&val);
                }
                _ => {
                    result.push('$');
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn read_until_close(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut expr = String::new();
    let mut depth = 1usize;
    for ch in chars.by_ref() {
        match ch {
            '{' => {
                depth += 1;
                expr.push(ch);
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                expr.push(ch);
            }
            _ => expr.push(ch),
        }
    }
    expr
}

fn read_plain_var(chars: &mut std::iter::Peekable<std::str::Chars>, first: char) -> String {
    let mut name = String::new();
    name.push(first);
    chars.next(); // consume the first char (already peeked)
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            name.push(c);
            chars.next();
        } else {
            break;
        }
    }
    name
}

fn expand_expr(expr: &str, env: &HashMap<String, String>) -> String {
    // ${VAR:-default} — use default when VAR is unset or empty
    if let Some(pos) = expr.find(":-") {
        let name = &expr[..pos];
        let default = &expr[pos + 2..];
        let val = lookup(name, env);
        return if val.is_empty() {
            default.to_owned()
        } else {
            val
        };
    }

    // ${VAR:+value} — use value when VAR is set and non-empty
    if let Some(pos) = expr.find(":+") {
        let name = &expr[..pos];
        let value = &expr[pos + 2..];
        let val = lookup(name, env);
        return if !val.is_empty() {
            value.to_owned()
        } else {
            String::new()
        };
    }

    // ${VAR} — plain lookup
    lookup(expr, env)
}

/// Look up a variable: check the provided env map first, then fall back to process env.
fn lookup(name: &str, env: &HashMap<String, String>) -> String {
    if let Some(v) = env.get(name) {
        return v.clone();
    }
    std::env::var(name).unwrap_or_default()
}

// ============ .env file loading ============

/// Parse a `.env` file into a key→value map.
///
/// Rules:
/// - Lines starting with `#` are comments
/// - Empty lines are skipped
/// - Format: `KEY=VALUE`, `KEY="VALUE"`, or `KEY='VALUE'`
/// - Inline `#` comments after unquoted values are stripped
pub fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, raw_val)) = line.split_once('=') {
            let key = key.trim().to_owned();
            if key.is_empty() {
                continue;
            }
            let val = parse_dotenv_value(raw_val.trim());
            map.insert(key, val);
        }
    }

    map
}

fn parse_dotenv_value(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }

    // Double-quoted: handle escape sequences
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        let inner = &raw[1..raw.len() - 1];
        return inner
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }

    // Single-quoted: literal, no escapes
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        return raw[1..raw.len() - 1].to_owned();
    }

    // Unquoted: strip inline comment (` #` or `\t#`)
    if let Some(pos) = raw.find(" #").or_else(|| raw.find("\t#")) {
        raw[..pos].trim_end().to_owned()
    } else {
        raw.to_owned()
    }
}

/// Load environment variables for compose interpolation.
///
/// Precedence (highest to lowest):
/// 1. Process environment (always wins)
/// 2. Explicit `--env-file` files (later files override earlier ones)
/// 3. Default `.env` file in `project_dir`
///
/// Returns a merged map where process env values are never overridden.
pub fn load_env(project_dir: &Path, extra_env_files: &[PathBuf]) -> HashMap<String, String> {
    // Start with an empty map — we'll layer values in reverse precedence order,
    // then let process env win at the end.
    let mut file_env: HashMap<String, String> = HashMap::new();

    // 1. Default .env in project directory (lowest priority among files)
    let default_env = project_dir.join(".env");
    if default_env.exists() {
        if let Ok(content) = std::fs::read_to_string(&default_env) {
            for (k, v) in parse_dotenv(&content) {
                file_env.entry(k).or_insert(v);
            }
        }
    }

    // 2. Explicit --env-file flags (later files override earlier ones)
    for ef in extra_env_files {
        if let Ok(content) = std::fs::read_to_string(ef) {
            for (k, v) in parse_dotenv(&content) {
                file_env.insert(k, v);
            }
        }
    }

    // 3. Process environment takes precedence over all file-based values
    let mut env = file_env;
    for (k, v) in std::env::vars() {
        env.insert(k, v);
    }

    env
}

// ============ YAML parsing ============

/// Parse a compose YAML string into a `ComposeSpec` after environment variable interpolation.
///
/// Returns a descriptive `ComposeError::ParseError` for malformed YAML.
pub fn parse_compose_yaml(yaml: &str, env: &HashMap<String, String>) -> Result<ComposeSpec> {
    let interpolated = interpolate_yaml(yaml, env);
    serde_yaml::from_str(&interpolated).map_err(ComposeError::ParseError)
}

// ============ Multi-file merge ============

/// Read, interpolate, parse, and merge multiple compose files in order.
///
/// Later files override earlier ones (last-writer-wins for all top-level maps).
/// Returns `ComposeError::FileNotFound` if any file is missing.
pub fn parse_and_merge_files(
    files: &[PathBuf],
    env: &HashMap<String, String>,
) -> Result<ComposeSpec> {
    let mut merged: Option<ComposeSpec> = None;

    for file_path in files {
        let content =
            std::fs::read_to_string(file_path).map_err(|_| ComposeError::FileNotFound {
                path: file_path.display().to_string(),
            })?;

        let spec = parse_compose_yaml(&content, env)?;

        match &mut merged {
            None => merged = Some(spec),
            Some(base) => base.merge(spec),
        }
    }

    Ok(merged.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- interpolate_yaml / interpolate ----

    #[test]
    fn test_interpolate_simple_braces() {
        let mut env = HashMap::new();
        env.insert("NAME".into(), "world".into());
        assert_eq!(interpolate_yaml("Hello ${NAME}!", &env), "Hello world!");
    }

    #[test]
    fn test_interpolate_plain_dollar() {
        let mut env = HashMap::new();
        env.insert("FOO".into(), "bar".into());
        assert_eq!(interpolate_yaml("$FOO baz", &env), "bar baz");
    }

    #[test]
    fn test_interpolate_default_when_missing() {
        let env = HashMap::new();
        assert_eq!(interpolate_yaml("${MISSING:-fallback}", &env), "fallback");
    }

    #[test]
    fn test_interpolate_default_when_empty() {
        let mut env = HashMap::new();
        env.insert("EMPTY".into(), "".into());
        assert_eq!(interpolate_yaml("${EMPTY:-fallback}", &env), "fallback");
    }

    #[test]
    fn test_interpolate_default_not_used_when_set() {
        let mut env = HashMap::new();
        env.insert("SET".into(), "value".into());
        assert_eq!(interpolate_yaml("${SET:-fallback}", &env), "value");
    }

    #[test]
    fn test_interpolate_conditional_set() {
        let mut env = HashMap::new();
        env.insert("SET".into(), "yes".into());
        assert_eq!(interpolate_yaml("${SET:+value}", &env), "value");
    }

    #[test]
    fn test_interpolate_conditional_unset() {
        let env = HashMap::new();
        assert_eq!(interpolate_yaml("${UNSET:+value}", &env), "");
    }

    #[test]
    fn test_interpolate_dollar_dollar_escape() {
        let env = HashMap::new();
        assert_eq!(interpolate_yaml("$$FOO", &env), "$FOO");
        assert_eq!(interpolate_yaml("price: $$9.99", &env), "price: $9.99");
    }

    #[test]
    fn test_interpolate_unknown_var_empty() {
        let env = HashMap::new();
        assert_eq!(interpolate_yaml("${UNKNOWN}", &env), "");
    }

    // ---- parse_dotenv ----

    #[test]
    fn test_parse_dotenv_basic() {
        let content = "FOO=bar\nBAZ=qux\n# comment\n\nEMPTY=";
        let map = parse_dotenv(content);
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
        assert_eq!(map["EMPTY"], "");
    }

    #[test]
    fn test_parse_dotenv_double_quoted() {
        let content = r#"A="hello world"
B="with \"escape\""
C="newline\nhere"
"#;
        let map = parse_dotenv(content);
        assert_eq!(map["A"], "hello world");
        assert_eq!(map["B"], "with \"escape\"");
        assert_eq!(map["C"], "newline\nhere");
    }

    #[test]
    fn test_parse_dotenv_single_quoted() {
        let content = "B='single quoted'\n";
        let map = parse_dotenv(content);
        assert_eq!(map["B"], "single quoted");
    }

    #[test]
    fn test_parse_dotenv_inline_comment() {
        let content = "KEY=value # this is a comment\n";
        let map = parse_dotenv(content);
        assert_eq!(map["KEY"], "value");
    }

    #[test]
    fn test_parse_dotenv_equals_in_value() {
        let content = "URL=http://example.com?a=1&b=2\n";
        let map = parse_dotenv(content);
        assert_eq!(map["URL"], "http://example.com?a=1&b=2");
    }

    // ---- parse_compose_yaml ----

    #[test]
    fn test_parse_compose_yaml_basic() {
        let yaml = r#"
services:
  web:
    image: nginx
"#;
        let env = HashMap::new();
        let spec = parse_compose_yaml(yaml, &env).unwrap();
        assert!(spec.services.contains_key("web"));
        assert_eq!(spec.services["web"].image.as_deref(), Some("nginx"));
    }

    #[test]
    fn test_parse_compose_yaml_with_interpolation() {
        let yaml = r#"
services:
  web:
    image: ${IMAGE:-nginx}
"#;
        let mut env = HashMap::new();
        env.insert("IMAGE".into(), "redis".into());
        let spec = parse_compose_yaml(yaml, &env).unwrap();
        assert_eq!(spec.services["web"].image.as_deref(), Some("redis"));

        // Default fallback
        let empty_env = HashMap::new();
        let spec2 = parse_compose_yaml(yaml, &empty_env).unwrap();
        assert_eq!(spec2.services["web"].image.as_deref(), Some("nginx"));
    }

    #[test]
    fn test_parse_compose_yaml_malformed_returns_error() {
        let yaml = "services: [unclosed";
        let env = HashMap::new();
        let result = parse_compose_yaml(yaml, &env);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ComposeError::ParseError(_)));
    }

    // ---- ComposeSpec::merge (via parse_and_merge_files logic) ----

    #[test]
    fn test_merge_last_writer_wins_services() {
        let yaml1 = r#"
services:
  web:
    image: nginx
  db:
    image: postgres
"#;
        let yaml2 = r#"
services:
  web:
    image: apache
"#;
        let env = HashMap::new();
        let mut spec1 = parse_compose_yaml(yaml1, &env).unwrap();
        let spec2 = parse_compose_yaml(yaml2, &env).unwrap();
        spec1.merge(spec2);

        // web overridden by second file
        assert_eq!(spec1.services["web"].image.as_deref(), Some("apache"));
        // db preserved from first file
        assert_eq!(spec1.services["db"].image.as_deref(), Some("postgres"));
    }

    #[test]
    fn test_merge_last_writer_wins_networks() {
        let yaml1 = r#"
services:
  web:
    image: nginx
networks:
  frontend:
    driver: bridge
"#;
        let yaml2 = r#"
services:
  api:
    image: node
networks:
  frontend:
    driver: overlay
  backend:
    driver: bridge
"#;
        let env = HashMap::new();
        let mut spec1 = parse_compose_yaml(yaml1, &env).unwrap();
        let spec2 = parse_compose_yaml(yaml2, &env).unwrap();
        spec1.merge(spec2);

        let nets = spec1.networks.as_ref().unwrap();
        // frontend overridden
        assert_eq!(
            nets["frontend"].as_ref().unwrap().driver.as_deref(),
            Some("overlay")
        );
        // backend added
        assert!(nets.contains_key("backend"));
    }

    // ---- parse_and_merge_files ----

    #[test]
    fn test_parse_and_merge_files_missing_returns_error() {
        let files = vec![PathBuf::from("/nonexistent/compose.yaml")];
        let env = HashMap::new();
        let result = parse_and_merge_files(&files, &env);
        assert!(matches!(
            result.unwrap_err(),
            ComposeError::FileNotFound { .. }
        ));
    }

    #[test]
    fn test_parse_and_merge_files_empty_returns_default() {
        let env = HashMap::new();
        let spec = parse_and_merge_files(&[], &env).unwrap();
        assert!(spec.services.is_empty());
    }
}

#[cfg(test)]
mod tests_v5 {
    use super::*;
    use proptest::prelude::*;

    // Feature: perry-container, Property 6: YAML round-trip (CLI path)
    proptest! {
        #[test]
        fn test_yaml_roundtrip(name in ".*", version in ".*") {
            let spec = ComposeSpec {
                name: Some(name),
                version: Some(version),
                ..Default::default()
            };
            let yaml_str = spec.to_yaml().unwrap();
            let de = ComposeSpec::parse_str(&yaml_str).unwrap();
            assert_eq!(spec.name, de.name);
            assert_eq!(spec.version, de.version);
        }
    }
}

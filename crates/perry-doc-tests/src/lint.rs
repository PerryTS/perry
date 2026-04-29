//! Markdown lint: every fenced ```typescript / ```ts code block in the scanned
//! tree must either be a pure `{{#include …}}` block or carry the `,no-test`
//! fence attribute. This keeps drift at bay: if a docs contributor embeds a
//! raw snippet, CI tells them to either extract it to a real `.ts` file or
//! mark it as illustrative-only.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub fence: String,
    pub first_body_line: String,
}

pub fn run(root: &Path) -> Result<Vec<Violation>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        scan_file(path, &text, &mut out);
    }
    out.sort_by(|a, b| (a.file.clone(), a.line).cmp(&(b.file.clone(), b.line)));
    Ok(out)
}

fn scan_file(path: &Path, text: &str, out: &mut Vec<Violation>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        if let Some(fence) = opening_typescript_fence(line) {
            // Skip if the fence already declares ,no-test (or ,no_test).
            if fence_has_notest(fence) {
                i = advance_past_close(&lines, i);
                continue;
            }
            // Walk to the first non-empty body line. If the entire body is a
            // single `{{#include ...}}` expression, it's compliant.
            let body_start = i + 1;
            let close_idx = find_close_fence(&lines, body_start);
            let body: &[&str] = &lines[body_start..close_idx];
            if body_is_pure_include(body) {
                i = close_idx + 1;
                continue;
            }
            let first_body_line = body
                .iter()
                .find(|l| !l.trim().is_empty())
                .copied()
                .unwrap_or("")
                .to_string();
            out.push(Violation {
                file: path.to_path_buf(),
                line: i + 1,
                fence: line.to_string(),
                first_body_line,
            });
            i = close_idx + 1;
        } else {
            i += 1;
        }
    }
}

fn opening_typescript_fence(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("```") {
        return None;
    }
    let info = trimmed.trim_start_matches("```");
    // info strings can be "typescript", "ts", "typescript,no-test", "ts ignore", etc.
    let head = info.split([',', ' ']).next().unwrap_or("").trim();
    if head.eq_ignore_ascii_case("typescript") || head.eq_ignore_ascii_case("ts") {
        Some(info.trim())
    } else {
        None
    }
}

fn fence_has_notest(info: &str) -> bool {
    info.split([',', ' '])
        .map(|t| t.trim())
        .any(|t| t.eq_ignore_ascii_case("no-test") || t.eq_ignore_ascii_case("no_test"))
}

fn find_close_fence(lines: &[&str], start: usize) -> usize {
    let mut i = start;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("```") {
            return i;
        }
        i += 1;
    }
    lines.len()
}

fn advance_past_close(lines: &[&str], open: usize) -> usize {
    find_close_fence(lines, open + 1) + 1
}

fn body_is_pure_include(body: &[&str]) -> bool {
    let non_empty: Vec<&&str> = body.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.len() != 1 {
        return false;
    }
    let s = non_empty[0].trim();
    s.starts_with("{{#include") && s.ends_with("}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_raw_typescript_fence() {
        let md = "some prose\n\n```typescript\nconst x = 1;\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].line, 3);
    }

    #[test]
    fn accepts_no_test_annotation() {
        let md = "```typescript,no-test\nconst x = 1;\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn accepts_pure_include() {
        let md = "```typescript\n{{#include ../../examples/ui/counter.ts}}\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn rejects_include_with_extra_content() {
        let md = "```typescript\n{{#include a.ts}}\nextra\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn ignores_other_languages() {
        let md = "```rust\nfn main() {}\n```\n\n```bash\nls\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn ts_alias_also_linted() {
        let md = "```ts\nconst x = 1\n```\n";
        let mut v = Vec::new();
        scan_file(Path::new("t.md"), md, &mut v);
        assert_eq!(v.len(), 1);
    }
}

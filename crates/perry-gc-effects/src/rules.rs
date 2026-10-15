use anyhow::{bail, Context, Result};
use perry_perex::{Budget, Limits, Regex};

pub const SEEDS: &str = include_str!("../../../scripts/gc_call_effects/seeds.txt");

pub struct Rule {
    pub kind: String,
    pub pattern: String,
    pub reason: String,
    pub line: usize,
    pub targets: Option<Box<Rule>>,
    matcher: Matcher,
}
enum Matcher {
    Exact,
    Prefix,
    Contains,
    Regex(Regex),
}

impl Rule {
    fn new(kind: &str, mt: &str, pattern: &str, reason: &str, line: usize) -> Result<Self> {
        let (pattern, targets) = if matches!(kind, "indirect" | "indirect-all" | "idiom") {
            if let Some((a, b)) = pattern.split_once(" => ") {
                let (m, p) = b
                    .trim()
                    .split_once(char::is_whitespace)
                    .context("missing target pattern")?;
                (
                    a.trim(),
                    Some(Box::new(Self::new("target", m, p.trim(), reason, line)?)),
                )
            } else {
                if kind == "idiom" {
                    bail!("seeds.txt:{line}: idiom must name its targets");
                }
                (pattern, None)
            }
        } else {
            (pattern, None)
        };
        let matcher = match mt {
            "exact" => Matcher::Exact,
            "prefix" => Matcher::Prefix,
            "contains" => Matcher::Contains,
            "regex" => Matcher::Regex(
                Regex::compile(
                    pattern.as_bytes(),
                    "u",
                    Limits::default(),
                    &mut Budget::new(1 << 40),
                )
                .with_context(|| format!("seeds.txt:{line}: {pattern}"))?,
            ),
            _ => bail!("seeds.txt:{line}: unknown match kind {mt}"),
        };
        Ok(Self {
            kind: kind.into(),
            pattern: pattern.into(),
            reason: reason.into(),
            line,
            targets,
            matcher,
        })
    }
    pub fn matches(&self, s: &str) -> bool {
        match &self.matcher {
            Matcher::Exact => s == self.pattern,
            Matcher::Prefix => s.starts_with(&self.pattern),
            Matcher::Contains => s.contains(&self.pattern),
            Matcher::Regex(rx) => rx
                .is_match(s.as_bytes(), Limits::default(), &mut Budget::new(1 << 40))
                .expect("rule search must complete; failure is never a no-match"),
        }
    }
    pub fn description(&self) -> String {
        format!("seeds.txt:{}: {} {}", self.line, self.kind, self.pattern)
    }
}
pub fn load_rules(text: &str) -> Result<Vec<Rule>> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let (head, reason) = line
            .split_once(" -- ")
            .with_context(|| format!("seeds.txt:{n}: every rule needs a reason"))?;
        if reason.trim().len() < 12 {
            bail!("seeds.txt:{n}: reason too short");
        }
        // split_whitespace cannot retain spaces in the pattern. Separate the
        // first two tokens explicitly, accepting arbitrary whitespace.
        let (kind, rest) = head
            .trim()
            .split_once(char::is_whitespace)
            .context("missing rule match")?;
        let (mt, pattern) = if kind == "idiom" {
            ("exact", rest.trim())
        } else {
            let (m, p) = rest
                .trim()
                .split_once(char::is_whitespace)
                .context("missing rule pattern")?;
            (m, p.trim())
        };
        if ![
            "collector",
            "poll",
            "js",
            "throw",
            "panic",
            "teardown",
            "nothrow",
            "unwind",
            "indirect",
            "indirect-all",
            "extern",
            "forbid",
            "trusted",
            "idiom",
            "closures",
            "target",
        ]
        .contains(&kind)
        {
            bail!("seeds.txt:{n}: unknown kind {kind}");
        }
        out.push(Rule::new(kind, mt, pattern, reason.trim(), n)?);
    }
    Ok(out)
}

//! Rust use-tree expansion for declaration body ownership.
use super::{info, tokens, Token};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Imports {
    aliases: BTreeMap<String, String>,
    globs: BTreeMap<String, Vec<String>>,
}
fn absolute(module: &str, reference: &str) -> String {
    let package = module.split("::").next().unwrap();
    if let Some(rest) = reference.strip_prefix("crate::") {
        return format!("{package}::{rest}");
    }
    let mut base = module;
    let mut rest = reference.strip_prefix("self::").unwrap_or(reference);
    while let Some(tail) = rest.strip_prefix("super::") {
        base = base.rsplit_once("::").map_or(package, |p| p.0);
        rest = tail;
    }
    if rest.starts_with("perry_") {
        rest.into()
    } else {
        format!("{base}::{rest}")
    }
}
impl Imports {
    fn tree(&mut self, module: &str, prefix: &str, ts: &[Token<'_>]) {
        if let Some(open) = ts.iter().position(|t| t.text == "{") {
            let head = format!("{prefix}{}", info::text(&ts[..open]));
            for child in info::split(&ts[open + 1..ts.len() - 1], ",") {
                self.tree(module, &head, child);
            }
            return;
        }
        let alias = ts.iter().position(|t| t.text == "as");
        let tail = info::text(&ts[..alias.unwrap_or(ts.len())]);
        let path = if tail == "self" {
            prefix.trim_end_matches("::").to_owned()
        } else {
            format!("{prefix}{tail}")
        };
        if let Some(target) = path.strip_suffix("::*") {
            self.globs
                .entry(module.into())
                .or_default()
                .push(absolute(module, target));
        } else {
            let local = alias
                .map(|i| info::text(&ts[i + 1..]))
                .unwrap_or_else(|| path.rsplit("::").next().unwrap().into());
            self.aliases
                .insert(format!("{module}::{local}"), absolute(module, &path));
        }
    }
    pub fn add(&mut self, module: &str, source: &str) -> Result<(), String> {
        let ts = tokens(source)?;
        let mut i = 0;
        while i < ts.len() {
            if ts[i].text == "use" {
                let end = i + ts[i..]
                    .iter()
                    .position(|t| t.text == ";")
                    .ok_or("unterminated use")?;
                self.tree(module, "", &ts[i + 1..end]);
                i = end + 1;
                continue;
            }
            if ts[i].text == "{" {
                let mut depth = 1;
                i += 1;
                while i < ts.len() && depth > 0 {
                    if ts[i].text == "{" {
                        depth += 1;
                    }
                    if ts[i].text == "}" {
                        depth -= 1;
                    }
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        Ok(())
    }
    pub fn expand(&self, path: &str) -> Vec<String> {
        let mut queue = vec![path.to_owned()];
        let mut seen = BTreeSet::new();
        let mut i = 0;
        while i < queue.len() && queue.len() < 512 {
            let current = queue[i].clone();
            i += 1;
            if !seen.insert(current.clone()) {
                continue;
            }
            let mut head = current.as_str();
            loop {
                let suffix = &current[head.len()..];
                if let Some(target) = self.aliases.get(head) {
                    let next = format!("{target}{suffix}");
                    if !seen.contains(&next) {
                        queue.push(next);
                    }
                    break;
                }
                if let Some((module, leaf)) = head.rsplit_once("::") {
                    if let Some(targets) = self.globs.get(module) {
                        for target in targets {
                            let next = format!("{target}::{leaf}{suffix}");
                            if !seen.contains(&next) {
                                queue.push(next);
                            }
                        }
                    }
                    head = module;
                } else {
                    break;
                }
            }
        }
        queue
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expands_group_alias_and_glob_without_reading_function_local_imports() {
        let mut imports = Imports::default();
        imports.add("perry_runtime::install", "use super::methods::{clear, child as other}; use super::shared::*; fn f(){ use fake::clear; }").unwrap();
        assert!(imports
            .expand("perry_runtime::install::clear")
            .contains(&"perry_runtime::methods::clear".into()));
        assert!(imports
            .expand("perry_runtime::install::other::call")
            .contains(&"perry_runtime::methods::child::call".into()));
        assert!(imports
            .expand("perry_runtime::install::next")
            .contains(&"perry_runtime::shared::next".into()));
    }
}

//! Item-level native namespace declarations (A2-DESIGN §5, §7.3):
//! `realm_decl! { namespace NM_X = "x" { names = [...]; members = { ... } } }`
//! beside the module's code. The item is the vtable static that carries the
//! module's keys; its cell is born by the native namespace birth, which the
//! birth block names with the entry `namespaces = native_namespaces`. The
//! producer reads both as source text and ties the cell to the vtable through
//! the owner the item states.
use super::{birth, info, tokens, Token};
use crate::realm_json::Value;
use std::collections::BTreeSet;

struct Item {
    owner: String,
    entry: String,
    source: String,
    path: String,
    line: usize,
    vtable: String,
}

#[derive(Default)]
pub(super) struct ItemNamespaces {
    births: Vec<(String, String)>,
    items: Vec<Item>,
}

/// The entry of a birth block that names it as the birth of the item-level
/// namespaces.
pub(super) fn is_reference(entry: &[Token<'_>]) -> bool {
    info::text(entry) == "namespaces=native_namespaces"
}

/// `crate::fs::namespace` for `perry-runtime/src/fs/namespace.rs`: the path
/// of the vtable static the item declares.
fn module_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    let rest = path
        .strip_prefix("perry-runtime/src/")
        .and_then(|rest| rest.strip_suffix(".rs"))
        .ok_or_else(|| format!("{path}: a namespace item lives in the runtime crate"))?;
    let rest = rest.strip_suffix("/mod").unwrap_or(rest);
    Ok(if rest == "lib" {
        "crate".into()
    } else {
        format!("crate::{}", rest.replace('/', "::"))
    })
}

impl ItemNamespaces {
    pub(super) fn reference(&mut self, expression: &[Token<'_>], path: &str) {
        self.births
            .push((info::text(expression), path.replace('\\', "/")));
    }

    /// One item: `namespace SYM = "owner" { [names = [...];] members = { ... } }`.
    /// Each member is `key;` or `key = (value);`, the value one paren group
    /// holding the birth block's right-hand side.
    pub(super) fn item(
        &mut self,
        ts: &[Token<'_>],
        source: &str,
        path: &str,
        line: usize,
    ) -> Result<(), String> {
        let shape =
            || format!("{path}:{line}: namespace item needs `namespace SYM = \"owner\" {{ ... }}`");
        if ts.len() < 6
            || ts[0].text != "namespace"
            || !super::rust_identifier(ts[1].text)
            || ts[2].text != "="
            || ts[4].text != "{"
            || ts.last().is_none_or(|t| t.text != "}")
        {
            return Err(shape());
        }
        let owner = ts[3].string.clone().ok_or_else(shape)?;
        let span = |ts: &[Token<'_>]| &source[ts[0].start..ts[ts.len() - 1].end];
        let mut names = None;
        let mut members = None;
        for clause in info::split(&ts[5..ts.len() - 1], ";") {
            match clause {
                [] => {}
                [name, eq, ..] if name.text == "names" && eq.text == "=" && names.is_none() => {
                    names = Some(span(&clause[2..]).to_owned());
                }
                [name, eq, open, .., close]
                    if name.text == "members"
                        && eq.text == "="
                        && open.text == "{"
                        && close.text == "}"
                        && members.is_none() =>
                {
                    let mut text = String::new();
                    for member in info::split(&clause[3..clause.len() - 1], ";") {
                        match member {
                            [] => {}
                            [key] => text.push_str(&format!("{}; ", key.text)),
                            [key, eq, open, value @ .., close]
                                if eq.text == "="
                                    && open.text == "("
                                    && close.text == ")"
                                    && !value.is_empty()
                                    && super::closing_paren(member, 2) == Ok(member.len() - 1) =>
                            {
                                text.push_str(&format!("{} = {}; ", key.text, span(value)));
                            }
                            _ => {
                                return Err(format!(
                                    "{path}:{line}: member `{}` needs `key;` or `key = (value);`",
                                    info::text(member)
                                ))
                            }
                        }
                    }
                    members = Some(text);
                }
                _ => {
                    return Err(format!(
                        "{path}:{line}: unknown namespace clause `{}`",
                        info::text(clause)
                    ))
                }
            }
        }
        let members =
            members.ok_or_else(|| format!("{path}:{line}: namespace item has no members"))?;
        let names = names
            .map(|names| format!(", names = {names}"))
            .unwrap_or_default();
        self.items.push(Item {
            entry: format!(
                "\"%NativeNamespace:{owner}%\"{names}, native_namespace = \"{owner}\", members = {{ {members}}}"
            ),
            vtable: format!("{}::{}", module_path(path)?, ts[1].text),
            owner,
            source: source.to_owned(),
            path: path.replace('\\', "/"),
            line,
        });
        Ok(())
    }

    /// Birth every item through the one referenced birth, after all files
    /// are read.
    pub(super) fn finish(self, all: &mut Vec<Value>) -> Result<(), String> {
        if self.items.is_empty() && self.births.is_empty() {
            return Ok(());
        }
        let [(expression, _)] = self.births.as_slice() else {
            return Err(format!(
                "namespace items need exactly one birth that names them (`namespaces = native_namespaces`), found {}: {:?}",
                self.births.len(),
                self.births.iter().map(|(_, path)| path).collect::<Vec<_>>()
            ));
        };
        let expression = tokens(expression)?;
        let mut owners = BTreeSet::new();
        for item in self.items {
            if !owners.insert(item.owner.clone()) {
                return Err(format!(
                    "{}:{}: namespace {} is declared twice",
                    item.path, item.line, item.owner
                ));
            }
            let entry = tokens(&item.entry)?;
            let mut declaration = birth::birth_at(&expression, &entry, &item.source)
                .map_err(|e| format!("{}:{}: {e}", item.path, item.line))?;
            let Value::Object(map) = &mut declaration else {
                unreachable!()
            };
            map.insert("source".into(), Value::String(item.path));
            map.insert("line".into(), Value::Number(item.line.to_string()));
            map.insert("exotic_vtable".into(), Value::String(item.vtable));
            all.push(declaration);
        }
        Ok(())
    }
}

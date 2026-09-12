//! GetSubstitution over original string spans. Named getters execute in order
//! even when a custom exec result overlaps a previously consumed match.
use super::perex_api as api;
use super::perex_match_search::subject;
use super::perex_replace_storage::{length, text, List, Pieces, Units};
use super::perex_runtime::{self as host, EngineError};
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::string::StringHeader;
use crate::value::{js_nanbox_string, TAG_UNDEFINED};
use perex::span::Span;
use perex::Budget;

pub(super) struct Substitution<'a, 's> {
    pub input: &'a RuntimeHandle<'s>,
    pub matched: &'a RuntimeHandle<'s>,
    pub position: usize,
    pub captures: &'a List<'s>,
    pub groups: Option<&'a RuntimeHandle<'s>>,
    pub template: &'a RuntimeHandle<'s>,
}

impl Substitution<'_, '_> {
    pub(super) fn append(
        &self,
        output: &mut Pieces<'_>,
        emit: bool,
        budget: &mut Budget,
    ) -> Result<(), EngineError> {
        let bound = subject(*self.template)?;
        let mut reader = Units::new(&bound)?;
        let n = length(self.template);
        let (mut i, mut literal) = (0, 0);
        while i < n {
            if reader.at(i, budget)? != b'$' as u16 || i + 1 == n {
                i += 1;
                continue;
            }
            let marker = reader.at(i + 1, budget)?;
            let mut next = i + 2;
            let scope = RuntimeHandleScope::new();
            let mut selected = None;
            let (source, start, end) = match marker {
                0x24 => (self.template, i, i + 1),
                0x26 => (self.matched, 0, length(self.matched)),
                0x60 => (self.input, 0, self.position),
                0x27 => (
                    self.input,
                    (self.position + length(self.matched)).min(length(self.input)),
                    length(self.input),
                ),
                0x30..=0x39 => {
                    let mut index = (marker - 0x30) as usize;
                    if next < n {
                        let second = reader.at(next, budget)?;
                        if (0x30..=0x39).contains(&second) {
                            let two = index * 10 + (second - 0x30) as usize;
                            if two > 0 && two <= self.captures.len() {
                                index = two;
                                next += 1;
                            }
                        }
                    }
                    if index == 0 || index > self.captures.len() {
                        i += 1;
                        continue;
                    }
                    let value = self.captures.get(index - 1);
                    if value.to_bits() != TAG_UNDEFINED {
                        selected =
                            Some(scope.root_string_ptr(
                                crate::value::js_get_string_pointer_unified(value)
                                    as *const StringHeader,
                            ));
                    }
                    // An unmatched capture contributes the empty span.
                    let source = selected.as_ref().unwrap_or(self.template);
                    (source, 0, selected.as_ref().map_or(0, length))
                }
                0x3c if self.groups.is_some() => {
                    let name_start = next;
                    while next < n && reader.at(next, budget)? != 0x3e {
                        next += 1;
                    }
                    if next == n {
                        i += 1;
                        continue;
                    }
                    let key = api::caught(|| {
                        super::perex_strings::copy_span(
                            &bound,
                            Span::new(name_start, next).unwrap(),
                            budget,
                            api::OUTPUT_BYTES,
                            api::QUANTUM,
                            &mut host::poll,
                        )
                    })??;
                    let key = scope.root_string_ptr(key);
                    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
                    let groups = self.groups.unwrap();
                    let value = api::caught(|| {
                        let key = key
                            .with_const_ptr::<StringHeader, _>(|key| js_nanbox_string(key as i64));
                        crate::proxy::js_reflect_get(
                            groups.get_nanbox_f64(),
                            key,
                            groups.get_nanbox_f64(),
                        )
                    });
                    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
                    let value = scope.root_nanbox_f64(value?);
                    if value.get_nanbox_f64().to_bits() != TAG_UNDEFINED {
                        selected = Some(text(&scope, &value)?);
                    }
                    next += 1;
                    let source = selected.as_ref().unwrap_or(self.template);
                    (source, 0, selected.as_ref().map_or(0, length))
                }
                _ => {
                    i += 1;
                    continue;
                }
            };
            if emit {
                output.append(self.template, literal, i, budget)?;
                output.append(source, start, end, budget)?;
            }
            i = next;
            literal = i;
        }
        if emit {
            output.append(self.template, literal, n, budget)?;
        }
        Ok(())
    }
}

//! Reach operands use the same unguarded receiver union as site verdicts.
use crate::derive::ReceiverValue;
use crate::gen::Gen;
use crate::model::*;
use perry_hir::realm_reach_scope::{Operand, Values};
use std::collections::BTreeMap;

mod chains;
mod values;
use chains::realm_layer;
use values::{known_chain_callee, known_own_callees, value_set};

#[derive(Clone, Debug)]
pub struct OperandFacts {
    pub module: u32,
    pub byte_offset: u32,
    pub operand: Operand,
    pub values: Vec<ReceiverValue>,
    pub prims: u16,
    pub scope: Values,
}

pub(crate) fn derive(g: &Gen) -> Vec<OperandFacts> {
    let m = &g.m;
    let mut nodes: BTreeMap<(u32, u32, Operand), Vec<NodeId>> = BTreeMap::new();
    for site in &g.read_sites {
        if site.byte_offset != 0 && !site.value_call {
            nodes
                .entry((
                    site.module,
                    site.byte_offset,
                    Operand::NamedReceiver(m.atoms[site.key as usize].clone()),
                ))
                .or_default()
                .push(site.recv);
        }
    }
    for site in &g.element_sites {
        if site.byte_offset != 0 {
            nodes
                .entry((site.module, site.byte_offset, Operand::Receiver))
                .or_default()
                .push(site.recv);
        }
    }
    for site in &g.call_sites_at {
        if site.offset == 0 || site.call == NO_CTX {
            continue;
        }
        if !site.spread {
            for (i, node) in site.args.iter().enumerate() {
                nodes
                    .entry((site.module, site.offset, Operand::Arg(i as u32)))
                    .or_default()
                    .push(*node);
                for from in 0..=i {
                    nodes
                        .entry((site.module, site.offset, Operand::Rest(from as u32)))
                        .or_default()
                        .push(*node);
                }
            }
        }
        let call = &m.calls[site.call as usize];
        if let ThisArg::Node(node) = call.this {
            nodes
                .entry((site.module, site.offset, Operand::This))
                .or_default()
                .push(node);
        }
        nodes
            .entry((site.module, site.offset, Operand::Result))
            .or_default()
            .push(call.ret);
        if let Some(callee) = site.callee.or_else(|| m.relays.get(&site.call).copied()) {
            nodes
                .entry((site.module, site.offset, Operand::Callee))
                .or_default()
                .push(callee);
        }
    }
    for site in &g.source_calls {
        for (i, node) in site.args.iter().enumerate() {
            nodes
                .entry((site.module, site.offset, Operand::Arg(i as u32)))
                .or_default()
                .push(*node);
            for from in 0..=i {
                nodes
                    .entry((site.module, site.offset, Operand::Rest(from as u32)))
                    .or_default()
                    .push(*node);
            }
        }
        nodes
            .entry((site.module, site.offset, Operand::Result))
            .or_default()
            .push(site.result);
    }
    let mut out: Vec<_> = nodes
        .into_iter()
        .map(|((module, byte_offset, operand), nodes)| {
            let (values, prims, mut scope) = value_set(m, &nodes, g.runtime_error);
            if operand == Operand::Callee {
                scope.top = !scope.known_callees;
            }
            if operand == Operand::Receiver {
                let elements: Vec<_> = g
                    .element_sites
                    .iter()
                    .filter(|s| s.module == module && s.byte_offset == byte_offset)
                    .collect();
                scope.numeric_key = !elements.is_empty()
                    && elements.iter().all(|s| {
                        let node = &m.nodes[s.idx as usize];
                        node.prim != 0
                            && node.prim & !(prim::NUM | prim::BIGINT) == 0
                            && node.pts.to_sorted().is_empty()
                    });
            }
            OperandFacts {
                module,
                byte_offset,
                operand,
                values,
                prims,
                scope,
            }
        })
        .collect();
    let mut methods: BTreeMap<(u32, u32), Vec<&crate::gen::ReadSite>> = BTreeMap::new();
    for site in &g.read_sites {
        if site.byte_offset != 0 && site.is_call && !site.value_call {
            methods
                .entry((site.module, site.byte_offset))
                .or_default()
                .push(site);
        }
    }
    for ((module, byte_offset), sites) in methods {
        let at = out.iter().position(|o| {
            o.module == module && o.byte_offset == byte_offset && o.operand == Operand::Callee
        });
        let raw_known = at.is_none_or(|i| out[i].scope.known_callees);
        let own_known = at.is_some_and(|i| out[i].scope.known_callees);
        let mut program_receivers = true;
        let known = raw_known
            && sites.iter().all(|site| {
                let (_, prims, receivers) = value_set(m, &[site.recv], g.runtime_error);
                program_receivers &= prims == 0
                    && receivers.chains.iter().all(|chain| {
                        chain.iter().any(|layer| {
                            layer.realm.is_none() && layer.own.contains(&m.atoms[site.key as usize])
                        })
                    });
                let key = &m.atoms[site.key as usize];
                !receivers.top
                    && receivers.chains.iter().all(|chain| {
                        let own_proof = own_known
                            || chain
                                .iter()
                                .filter(|layer| layer.realm.is_none())
                                .all(|layer| {
                                    layer.token.is_some_and(|token| {
                                        known_own_callees(m, token, site.key, g.runtime_error)
                                    })
                                });
                        own_proof && known_chain_callee(chain, key, own_proof)
                    })
                    && receivers
                        .primitive_prototypes
                        .iter()
                        .all(|name| known_chain_callee(&[realm_layer(name)], key, false))
            });
        if let Some(i) = at {
            out[i].scope.top = !known;
            out[i].scope.known_callees = known;
            out[i].scope.program_bodies &= known && program_receivers;
        } else {
            out.push(OperandFacts {
                module,
                byte_offset,
                operand: Operand::Callee,
                values: Vec::new(),
                prims: 0,
                scope: Values {
                    top: !known,
                    known_callees: known,
                    ..Default::default()
                },
            });
        }
    }
    for site in &g.source_calls {
        out.push(OperandFacts {
            module: site.module,
            byte_offset: site.offset,
            operand: Operand::Callee,
            values: Vec::new(),
            prims: 0,
            scope: Values {
                known_callees: true,
                chains: vec![vec![realm_layer(site.realm_identity)]],
                ..Default::default()
            },
        });
    }
    out
}

impl crate::ProgramFacts {
    pub fn realm_operand_index(&self) -> perry_hir::realm_reach_scope::OperandIndex {
        let mut index = perry_hir::realm_reach_scope::OperandIndex {
            complete: self.realm_solve_complete
                && self.interp_guard.is_none()
                && self.realm_builtin_writes.is_empty(),
            ..Default::default()
        };
        for operand in &self.operands {
            if let Some(source) = self.module_paths.get(operand.module as usize) {
                let key = (source.clone(), operand.byte_offset, operand.operand.clone());
                index
                    .sites
                    .entry(key)
                    .and_modify(|current| {
                        current.top |= operand.scope.top;
                        current.program_bodies &= operand.scope.program_bodies;
                        current.known_callees &= operand.scope.known_callees;
                        current.numeric_key &= operand.scope.numeric_key;
                        current.chains.extend(operand.scope.chains.iter().cloned());
                        current
                            .primitive_prototypes
                            .extend(operand.scope.primitive_prototypes.iter().cloned());
                    })
                    .or_insert_with(|| operand.scope.clone());
            }
        }
        index
    }
}

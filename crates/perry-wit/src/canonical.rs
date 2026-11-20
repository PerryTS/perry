//! Canonical memory metadata for dynamically typed async payload adapters.
//!
//! Rebuild the validated contract graph in dependency order and delegate every
//! size, alignment, field offset and variant payload offset to wit-parser.
//! Neither the host nor its Wasm helper has to invent an aggregate layout.
use crate::{Type, World};
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use wit_parser::{Int, Resolve, SizeAlign, Type as Wit, TypeDef, TypeDefKind as Kind, TypeId};

#[cfg(test)]
#[path = "canonical_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub struct Layout {
    node_id: u32,
    pub size: u32,
    pub align: u32,
    pub kind: LayoutKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    Scalar,
    String,
    List {
        element: Arc<Layout>,
    },
    Fields(Vec<(u32, Arc<Layout>)>),
    Variant {
        tag_bytes: u32,
        payload_offset: u32,
        cases: Vec<Option<Arc<Layout>>>,
    },
}

impl Layout {
    /// Identity within this layout plan, shared by references to the same node.
    /// Generated visitors can memoize nodes without expanding shared aliases.
    pub fn node_id(&self) -> u32 {
        self.node_id
    }
}

// Node numbering belongs to one reconstruction, not the semantic ABI layout.
impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.align == other.align && self.kind == other.kind
    }
}
impl Eq for Layout {}

/// Native helper memories use the wasm32 canonical ABI, independently of the
/// host pointer width. This accepts a value type, not a bare resource type.
pub fn layout(world: &World, ty: &Type) -> Result<Arc<Layout>> {
    let mut graph = Graph {
        world,
        resolve: Resolve::default(),
        ids: BTreeMap::new(),
        active: BTreeSet::new(),
        bounds: Vec::new(),
    };
    let ty = graph.value(ty, 0)?;
    let mut sizes = SizeAlign::default();
    sizes.fill(&graph.resolve);
    describe(&graph.resolve, &sizes, ty, 0)
}

struct Graph<'a> {
    world: &'a World,
    resolve: Resolve,
    ids: BTreeMap<String, TypeId>,
    active: BTreeSet<String>,
    // A deliberately loose arithmetic bound, not an ABI size. It prevents
    // usize overflow in SizeAlign for exponentially expanded hostile graphs.
    // Zero-sized aggregates remain zero; variants use max, not sum of cases.
    bounds: Vec<u128>,
}

impl Graph<'_> {
    fn bound(&self, ty: Wit) -> u128 {
        match ty {
            Wit::Id(id) => self.bounds[id.index()],
            _ => 1,
        }
    }

    fn alloc(&mut self, kind: Kind) -> Result<Wit> {
        let bound = match &kind {
            Kind::Type(ty) => self.bound(*ty),
            Kind::Record(r) => r
                .fields
                .iter()
                .fold(0u128, |n, f| n.saturating_add(self.bound(f.ty))),
            Kind::Tuple(t) => t
                .types
                .iter()
                .fold(0u128, |n, t| n.saturating_add(self.bound(*t))),
            Kind::Variant(v) => 1u128.saturating_add(
                v.cases
                    .iter()
                    .filter_map(|c| c.ty)
                    .map(|t| self.bound(t))
                    .max()
                    .unwrap_or(0),
            ),
            Kind::Option(t) => 1u128.saturating_add(self.bound(*t)),
            Kind::Result(r) => 1u128.saturating_add(
                r.ok.into_iter()
                    .chain(r.err)
                    .map(|t| self.bound(t))
                    .max()
                    .unwrap_or(0),
            ),
            _ => 1,
        };
        ensure!(
            bound <= (usize::MAX / 16) as u128,
            "canonical type graph exceeds address arithmetic limits"
        );
        let id = self.resolve.types.alloc(TypeDef {
            name: None,
            kind,
            owner: wit_parser::TypeOwner::None,
            docs: Default::default(),
            stability: wit_parser::Stability::Unknown,
            span: Default::default(),
            external_id: None,
        });
        self.bounds.push(bound);
        Ok(Wit::Id(id))
    }

    fn definition(&mut self, key: &str, depth: usize) -> Result<Wit> {
        ensure!(depth < 128, "canonical type nesting exceeds 128");
        if let Some(id) = self.ids.get(key) {
            return Ok(Wit::Id(*id));
        }
        ensure!(
            self.active.insert(key.into()),
            "cyclic canonical type {key}"
        );
        let definition = self
            .world
            .types
            .get(key)
            .with_context(|| format!("unknown WIT type {key}"))?;
        let ty = if matches!(definition.ty, Type::Resource) {
            self.alloc(Kind::Resource)?
        } else {
            let ty = self.value(&definition.ty, depth + 1)?;
            self.alloc(Kind::Type(ty))?
        };
        let Wit::Id(id) = ty else { unreachable!() };
        self.ids.insert(key.into(), id);
        self.active.remove(key);
        Ok(ty)
    }

    fn resource(&mut self, key: &str, depth: usize) -> Result<TypeId> {
        ensure!(depth < 128, "canonical resource alias nesting exceeds 128");
        let definition = self
            .world
            .types
            .get(key)
            .with_context(|| format!("unknown resource type {key}"))?;
        match &definition.ty {
            Type::Ref(key) => self.resource(key, depth + 1),
            Type::Resource => match self.definition(key, depth + 1)? {
                Wit::Id(id) => Ok(id),
                _ => unreachable!(),
            },
            _ => bail!("expected a resource type for {key}"),
        }
    }

    fn value(&mut self, ty: &Type, depth: usize) -> Result<Wit> {
        ensure!(depth < 128, "canonical value nesting exceeds 128");
        Ok(match ty {
            Type::Bool => Wit::Bool,
            Type::U8 => Wit::U8,
            Type::U16 => Wit::U16,
            Type::U32 => Wit::U32,
            Type::U64 => Wit::U64,
            Type::S8 => Wit::S8,
            Type::S16 => Wit::S16,
            Type::S32 => Wit::S32,
            Type::S64 => Wit::S64,
            Type::F32 => Wit::F32,
            Type::F64 => Wit::F64,
            Type::Char => Wit::Char,
            Type::String => Wit::String,
            Type::Resource => bail!("a bare resource is not a canonical value"),
            Type::Ref(key) => {
                let ty = self.definition(key, depth)?;
                if let Wit::Id(id) = ty {
                    ensure!(
                        !matches!(self.resolve.types[id].kind, Kind::Resource),
                        "a bare resource is not a canonical value"
                    );
                }
                ty
            }
            Type::Own(key) | Type::Borrow(key) => {
                let id = self.resource(key, depth)?;
                self.alloc(Kind::Handle(if matches!(ty, Type::Own(_)) {
                    wit_parser::Handle::Own(id)
                } else {
                    wit_parser::Handle::Borrow(id)
                }))?
            }
            Type::List(ty) => {
                let ty = self.value(ty, depth + 1)?;
                self.alloc(Kind::List(ty))?
            }
            Type::Future(payload) | Type::Stream(payload) => {
                let payload = payload
                    .as_deref()
                    .map(|t| self.value(t, depth + 1))
                    .transpose()?;
                self.alloc(if matches!(ty, Type::Future(_)) {
                    Kind::Future(payload)
                } else {
                    Kind::Stream(payload)
                })?
            }
            Type::Record(fields) => {
                let fields = fields
                    .iter()
                    .map(|f| {
                        Ok(wit_parser::Field {
                            name: f.name.clone(),
                            ty: self.value(&f.ty, depth + 1)?,
                            docs: Default::default(),
                            span: Default::default(),
                        })
                    })
                    .collect::<Result<_>>()?;
                self.alloc(Kind::Record(wit_parser::Record { fields }))?
            }
            Type::Tuple(types) => {
                let types = types
                    .iter()
                    .map(|t| self.value(t, depth + 1))
                    .collect::<Result<_>>()?;
                self.alloc(Kind::Tuple(wit_parser::Tuple { types }))?
            }
            Type::Flags(names) => {
                ensure!(
                    (1..=32).contains(&names.len()),
                    "canonical flags require 1..=32 labels"
                );
                self.alloc(Kind::Flags(wit_parser::Flags {
                    flags: names
                        .iter()
                        .map(|n| wit_parser::Flag {
                            name: n.clone(),
                            docs: Default::default(),
                            span: Default::default(),
                        })
                        .collect(),
                }))?
            }
            Type::Enum(names) => {
                ensure!(!names.is_empty(), "canonical enum must have cases");
                self.alloc(Kind::Enum(wit_parser::Enum {
                    cases: names
                        .iter()
                        .map(|n| wit_parser::EnumCase {
                            name: n.clone(),
                            docs: Default::default(),
                            span: Default::default(),
                        })
                        .collect(),
                }))?
            }
            Type::Variant(cases) => {
                ensure!(!cases.is_empty(), "canonical variant must have cases");
                let cases = cases
                    .iter()
                    .map(|c| {
                        Ok(wit_parser::Case {
                            name: c.name.clone(),
                            ty: c
                                .ty
                                .as_ref()
                                .map(|t| self.value(t, depth + 1))
                                .transpose()?,
                            docs: Default::default(),
                            span: Default::default(),
                        })
                    })
                    .collect::<Result<_>>()?;
                self.alloc(Kind::Variant(wit_parser::Variant { cases }))?
            }
            Type::Option(ty) => {
                let ty = self.value(ty, depth + 1)?;
                self.alloc(Kind::Option(ty))?
            }
            Type::Result { ok, err } => {
                let ok = ok
                    .as_deref()
                    .map(|t| self.value(t, depth + 1))
                    .transpose()?;
                let err = err
                    .as_deref()
                    .map(|t| self.value(t, depth + 1))
                    .transpose()?;
                self.alloc(Kind::Result(wit_parser::Result_ { ok, err }))?
            }
        })
    }
}

fn describe(resolve: &Resolve, sizes: &SizeAlign, ty: Wit, depth: usize) -> Result<Arc<Layout>> {
    Describer {
        resolve,
        sizes,
        cache: BTreeMap::new(),
    }
    .value(ty, depth)
}

struct Describer<'a> {
    resolve: &'a Resolve,
    sizes: &'a SizeAlign,
    cache: BTreeMap<TypeId, Arc<Layout>>,
}

impl Describer<'_> {
    fn variant(&mut self, tag: Int, cases: Vec<Option<Wit>>, depth: usize) -> Result<LayoutKind> {
        Ok(LayoutKind::Variant {
            tag_bytes: match tag {
                Int::U8 => 1,
                Int::U16 => 2,
                Int::U32 => 4,
                Int::U64 => 8,
            },
            payload_offset: u32::try_from(
                self.sizes
                    .payload_offset(tag, cases.iter().map(Option::as_ref))
                    .size_wasm32(),
            )?,
            cases: cases
                .into_iter()
                .map(|ty| ty.map(|ty| self.value(ty, depth + 1)).transpose())
                .collect::<Result<_>>()?,
        })
    }

    fn fields(&mut self, types: Vec<Wit>, depth: usize) -> Result<LayoutKind> {
        Ok(LayoutKind::Fields(
            self.sizes
                .field_offsets(types.iter())
                .into_iter()
                .map(|(offset, ty)| {
                    Ok((
                        u32::try_from(offset.size_wasm32())?,
                        self.value(*ty, depth + 1)?,
                    ))
                })
                .collect::<Result<_>>()?,
        ))
    }

    fn value(&mut self, ty: Wit, depth: usize) -> Result<Arc<Layout>> {
        ensure!(depth < 128, "canonical layout nesting exceeds 128");
        if let Wit::Id(id) = ty {
            if let Some(layout) = self.cache.get(&id) {
                return Ok(Arc::clone(layout));
            }
        }
        // Reject oversized inline aggregates before descending into them. A
        // small alias DAG can describe more than four gigabytes of values.
        let size = u32::try_from(self.sizes.size(&ty).size_wasm32())
            .context("canonical value exceeds wasm32 size limits")?;
        let align = u32::try_from(self.sizes.align(&ty).align_wasm32())?;
        let (node_id, kind) = match ty {
            Wit::Id(id) => {
                let node_id = u32::try_from(id.index())?
                    .checked_add(32)
                    .context("canonical layout has too many nodes")?;
                let kind = match &self.resolve.types[id].kind {
                    Kind::Type(ty) => {
                        let layout = self.value(*ty, depth)?;
                        self.cache.insert(id, Arc::clone(&layout));
                        return Ok(layout);
                    }
                    Kind::List(ty) => LayoutKind::List {
                        element: self.value(*ty, depth + 1)?,
                    },
                    Kind::Record(r) => {
                        self.fields(r.fields.iter().map(|f| f.ty).collect(), depth)?
                    }
                    Kind::Tuple(t) => self.fields(t.types.clone(), depth)?,
                    Kind::Variant(v) => {
                        self.variant(v.tag(), v.cases.iter().map(|c| c.ty).collect(), depth)?
                    }
                    Kind::Option(t) => self.variant(Int::U8, vec![None, Some(*t)], depth)?,
                    Kind::Result(r) => self.variant(Int::U8, vec![r.ok, r.err], depth)?,
                    Kind::Resource => bail!("a bare resource is not a canonical value"),
                    Kind::Handle(_)
                    | Kind::Future(_)
                    | Kind::Stream(_)
                    | Kind::Flags(_)
                    | Kind::Enum(_) => LayoutKind::Scalar,
                    _ => bail!("unsupported canonical layout"),
                };
                (node_id, kind)
            }
            Wit::Bool => (0, LayoutKind::Scalar),
            Wit::U8 => (1, LayoutKind::Scalar),
            Wit::S8 => (2, LayoutKind::Scalar),
            Wit::U16 => (3, LayoutKind::Scalar),
            Wit::S16 => (4, LayoutKind::Scalar),
            Wit::U32 => (5, LayoutKind::Scalar),
            Wit::S32 => (6, LayoutKind::Scalar),
            Wit::U64 => (7, LayoutKind::Scalar),
            Wit::S64 => (8, LayoutKind::Scalar),
            Wit::F32 => (9, LayoutKind::Scalar),
            Wit::F64 => (10, LayoutKind::Scalar),
            Wit::Char => (11, LayoutKind::Scalar),
            Wit::String => (12, LayoutKind::String),
            _ => bail!("unsupported canonical primitive"),
        };
        let layout = Arc::new(Layout {
            node_id,
            size,
            align,
            kind,
        });
        if let Wit::Id(id) = ty {
            self.cache.insert(id, Arc::clone(&layout));
        }
        Ok(layout)
    }
}

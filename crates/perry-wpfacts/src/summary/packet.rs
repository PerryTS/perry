//! Body-owned generation input. The builder consumes explicit operations;
//! it cannot inspect solved values, other summaries or the reach set.
use crate::model::*;
use std::rc::Rc;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Identity(pub String, pub u32, pub String);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Cell {
    pub owner: Identity,
    pub local: u32,
}

/// The lexical declaration shape exists before any child demands a port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Environment {
    pub owner: Identity,
    pub cells: Vec<u32>,
    pub parent: Option<Rc<Environment>>,
}

impl Environment {
    fn cell(&self, local: u32) -> Option<Cell> {
        if self.cells.binary_search(&local).is_ok() {
            Some(Cell {
                owner: self.owner.clone(),
                local,
            })
        } else {
            self.parent.as_ref().and_then(|parent| parent.cell(local))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Value {
    pub identity: String,
    pub environment: Option<Rc<Environment>>,
    pub context: Option<EnvironmentPort>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentPort {
    Current,
    Outer(usize),
    Shared,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FieldPort {
    Named(String),
    Any,
    NamedAny,
    All,
    Numeric,
    NumberKeys,
    Private(String),
    Hole,
    Prototype,
}

#[derive(Clone, Debug)]
pub(crate) struct FieldOrigin {
    pub object: Option<Tok>,
    pub key: FieldPort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Term {
    Cell(Cell),
    Environment(Cell),
    Intrinsic(String),
    Interface(Identity, Binding),
    Local(u32),
    External(u32),
    Field(Value, FieldPort),
    Absent,
}

/// Header ports describe entry binding, including conservative shared entry.
/// No parameter type or current parameter value is part of this shape.
#[derive(Clone, Debug, Default)]
pub(crate) struct Header {
    pub identity: Identity,
    pub parameters: Vec<(u32, bool, Option<bool>)>,
    pub shared: Vec<SharedEntry>,
    pub environment: Option<Rc<Environment>>,
    pub lexical_receiver: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum SharedEntry {
    Rest {
        local: u32,
        start: usize,
    },
    Arguments {
        local: u32,
        strict: bool,
        mapped: Vec<(u32, u32)>,
        restricted: bool,
    },
    ComputedAccessor {
        class: String,
    },
    External,
}

#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Seed(Term, Vec<Value>, u16),
    Edge(Term, Term),
    Rule(Term, Cons, String, Vec<Term>, Vec<CallPort>),
    LoadFrom(Value, Value, String, Term),
    Activate(Option<Identity>, String),
    Escape(String, String),
    ArrayMemberWrite(String),
    SharedBirth(Identity, Term),
    GeneratorPrototype {
        function: Identity,
        object: Term,
        is_async: bool,
    },
    ClassBinding {
        module: String,
        name: String,
        action: ClassAction<Term>,
    },
}

/// Resolution belongs to application; incoming class values cannot choose
/// which operation a body emits. Names here are canonical HIR bindings.
#[derive(Clone, Debug)]
pub(crate) enum ClassAction<T> {
    DynamicParent(T),
    ComputedMethod {
        key: T,
        method: String,
        is_static: bool,
    },
    ComputedAccessor {
        key: T,
        getter: Option<String>,
        setter: Option<String>,
        is_static: bool,
        named: bool,
    },
    PerEvaluation,
    EvaluatedParent(T),
    PrototypeMethod {
        key: String,
        value: T,
    },
}

impl<T> ClassAction<T> {
    pub(crate) fn map<U>(self, mut f: impl FnMut(T) -> U) -> ClassAction<U> {
        match self {
            Self::DynamicParent(v) => ClassAction::DynamicParent(f(v)),
            Self::ComputedMethod {
                key,
                method,
                is_static,
            } => ClassAction::ComputedMethod {
                key: f(key),
                method,
                is_static,
            },
            Self::ComputedAccessor {
                key,
                getter,
                setter,
                is_static,
                named,
            } => ClassAction::ComputedAccessor {
                key: f(key),
                getter,
                setter,
                is_static,
                named,
            },
            Self::PerEvaluation => ClassAction::PerEvaluation,
            Self::EvaluatedParent(v) => ClassAction::EvaluatedParent(f(v)),
            Self::PrototypeMethod { key, value } => ClassAction::PrototypeMethod {
                key,
                value: f(value),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CallPort {
    pub present: bool,
    pub this: Option<Term>,
    pub this_value: Option<Value>,
    pub args: Vec<Term>,
    pub spread: Vec<Term>,
    pub ret: Term,
}

/// Runtime links are supplied by the legacy publication adapter. They are
/// ports, never a snapshot of the values those nodes happen to hold.
#[derive(Clone, Debug)]
pub(crate) enum Emission {
    Seed(NodeId, Vec<Tok>, u16),
    Edge(NodeId, NodeId),
    Rule(NodeId, Cons),
    LoadFrom(Tok, Tok, Key, NodeId),
    Activate(FnIdx, String),
    Escape(Tok, u32),
    ArrayMemberWrite(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BirthKind {
    Class {
        name: String,
        id: u32,
        literal: bool,
    },
    Literal {
        enum_image: bool,
    },
    FunctionInstance(Identity),
    ObjectCreate,
    /// An Array birth: both bits of its template (`TmplKind::Arr`) are
    /// part of the birth, so a packet's birth names a plain or hole-free
    /// template as such (REVIEW-p4l1 R2).
    Array {
        hole_free: bool,
        plain: bool,
    },
    Arguments,
    Builtin(&'static str),
}

#[derive(Clone, Debug)]
pub(crate) struct Birth {
    pub template: TmplId,
    pub module: String,
    pub kind: Option<BirthKind>,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Dormant,
    Building,
    Ready,
}

#[derive(Clone, Debug)]
pub(crate) struct Observation {
    pub offset: u32,
    pub kind: String,
    pub key: String,
    pub receiver: Term,
    pub index: Option<Term>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BodyPacket {
    pub header: Header,
    pub cells: Vec<Cell>,
    pub operations: Vec<Operation>,
    pub observations: Vec<Observation>,
    pub links: Vec<NodeId>,
    pub emissions: Vec<Emission>,
    pub status: Status,
    pub formals: Vec<Formal>,
    pub births: Vec<Birth>,
}

/// Entry bindings remain distinct from writable lexical cells. A caller
/// supplies a binding port; application delivers it into the addressed cell.
#[derive(Clone, Debug)]
pub(crate) struct Formal {
    pub binding: Binding,
    pub target: Term,
    pub from_environment: Option<Term>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    Argument(usize),
    Receiver,
    Result,
    AllArguments,
}

impl Binding {
    fn bytes(&self) -> String {
        match self {
            Self::Argument(i) => format!("argument {i}"),
            Self::Receiver => "receiver".into(),
            Self::Result => "result".into(),
            Self::AllArguments => "all arguments".into(),
        }
    }
}

impl Operation {
    fn bytes(&self) -> String {
        match self {
            Self::Seed(n, ts, p) => format!(
                "seed {n:?} {:?} {p}",
                ts.iter()
                    .map(|t| (&t.identity, &t.environment, &t.context))
                    .collect::<Vec<_>>()
            ),
            Self::Edge(a, b) => format!("edge {a:?} {b:?}"),
            Self::LoadFrom(origin, holder, key, dst) => {
                format!("load-from {origin:?} {holder:?} {key} {dst:?}")
            }
            Self::Activate(f, path) => format!("activate {f:?} {path}"),
            Self::Escape(value, reason) => format!("escape {value} {reason}"),
            Self::ArrayMemberWrite(reason) => format!("array-write {reason}"),
            Self::ClassBinding {
                module,
                name,
                action,
            } => format!("class binding {module:?} {name:?} {action:?}"),
            Self::GeneratorPrototype {
                function,
                object,
                is_async,
            } => format!("generator prototype {function:?} {object:?} async={is_async}"),
            Self::SharedBirth(body, value) => format!("shared-birth {body:?} {value:?}"),
            Self::Rule(n, rule, c, terms, calls) => {
                let _kind = rule.kind_name();
                format!(
                    "rule {n:?} {c} {terms:?} {:?}",
                    calls.iter().map(CallPort::bytes).collect::<Vec<_>>()
                )
            }
        }
    }
}

impl CallPort {
    fn bytes(&self) -> String {
        format!(
            "present={} this={:?} this_value={:?} args={:?} spread={:?} ret={:?}",
            self.present, self.this, self.this_value, self.args, self.spread, self.ret
        )
    }
}

impl SharedEntry {
    fn bytes(&self) -> String {
        match self {
            Self::Rest { local, start } => format!("rest {local} from {start}"),
            Self::Arguments {
                local,
                strict,
                mapped,
                restricted,
            } => format!("arguments {local} {strict} {mapped:?} {restricted}"),
            Self::ComputedAccessor { class } => {
                format!("computed accessor receiver {class}; result to shared prototype outlet")
            }
            Self::External => "shared entry/receiver/result".into(),
        }
    }
}

impl BodyPacket {
    pub(crate) fn ready(&self) -> bool {
        self.status == Status::Ready
    }

    pub(crate) fn bytes(&self) -> String {
        // Linking addresses are deliberately outside the immutable transfer.
        format!(
            "{:?} {:?} {:?} {:?} {:?}\n{:?}\n{:?}\n{:?}",
            self.header.identity,
            self.header.environment,
            self.header.lexical_receiver,
            self.header.parameters,
            self.header
                .shared
                .iter()
                .map(SharedEntry::bytes)
                .collect::<Vec<_>>(),
            (
                self.cells.clone(),
                self.births
                    .iter()
                    .map(|b| (&b.module, &b.kind, &b.keys))
                    .collect::<Vec<_>>()
            ),
            self.formals
                .iter()
                .map(|f| (f.binding.bytes(), &f.target, &f.from_environment))
                .collect::<Vec<_>>(),
            (
                self.operations
                    .iter()
                    .map(Operation::bytes)
                    .collect::<Vec<_>>(),
                self.observations
                    .iter()
                    .map(|o| (o.offset, &o.kind, &o.key, &o.receiver, &o.index))
                    .collect::<Vec<_>>()
            )
        )
    }

    pub(crate) fn publish(&self, model: &mut Model) {
        assert!(self.ready(), "publication requires a sealed body");
        assert!(model.packet_scope.is_none());
        for op in &self.emissions {
            match op {
                Emission::Seed(n, ts, p) => model.add(*n, ts, *p),
                Emission::Edge(a, b) => model.edge(*a, *b),
                Emission::Rule(n, c) => model.cons(*n, *c),
                Emission::LoadFrom(origin, holder, key, dst) => {
                    model.load_from(*origin, *holder, *key, *dst)
                }
                Emission::Activate(f, path) => model.activate(*f, path),
                Emission::Escape(t, reason) => model.escape(*t, *reason),
                Emission::ArrayMemberWrite(reason) => {
                    model.array_member_write.get_or_insert(*reason);
                }
            }
        }
    }
}

pub(crate) struct Builder {
    pub f: FnIdx,
    pub inert: bool,
    pub packet: BodyPacket,
}

impl Builder {
    fn term(&mut self, node: NodeId, cell: Option<Cell>, local: bool) -> Term {
        if node == NO_NODE {
            return Term::Absent;
        }
        if let Some(cell) = cell {
            return if cell.owner == self.packet.header.identity {
                Term::Cell(cell)
            } else {
                Term::Environment(cell)
            };
        }
        let i = match self.packet.links.iter().position(|n| *n == node) {
            Some(i) => i,
            None => {
                self.packet.links.push(node);
                self.packet.links.len() - 1
            }
        };
        if local {
            Term::Local(i as u32)
        } else {
            Term::External(i as u32)
        }
    }

    fn finish(mut self, seal: bool) -> BodyPacket {
        self.packet.cells.sort();
        self.packet.cells.dedup();
        assert!(
            self.packet.births.iter().all(|b| b.kind.is_some()),
            "all direct births have owned definitions"
        );
        self.packet.status = if seal { Status::Ready } else { Status::Dormant };
        self.packet
    }
}

mod emission;
mod symbols;

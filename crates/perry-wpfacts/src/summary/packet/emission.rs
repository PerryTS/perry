//! Test adapter: read structural operands at emission, never solved contents.
use super::*;

impl Model {
    pub(crate) fn packet_header(
        &mut self,
        f: FnIdx,
        identity: Identity,
        params: &[perry_hir::Param],
    ) -> Option<Builder> {
        if !self.packets_enabled {
            return None;
        }
        let mut declared: Vec<_> = params.iter().map(|p| p.id).collect();
        declared.sort_unstable();
        declared.dedup();
        let environment = Rc::new(Environment {
            owner: identity.clone(),
            cells: declared,
            parent: self
                .packet_scope
                .as_ref()
                .and_then(|b| b.packet.header.environment.clone())
                .or_else(|| {
                    self.funcs
                        .get(self.cur_fn as usize)
                        .and_then(|f| f.body_packet.header.environment.clone())
                }),
        });
        let mut header = Header {
            identity,
            environment: Some(environment),
            shared: vec![SharedEntry::External],
            ..Header::default()
        };
        let mut positional = 0;
        for p in params {
            header.parameters.push((
                p.id,
                p.is_rest,
                p.arguments_object.as_ref().map(|a| a.strict),
            ));
            if p.is_rest {
                header.shared.push(SharedEntry::Rest {
                    local: p.id,
                    start: positional,
                });
            }
            if !p.is_rest && p.arguments_object.is_none() {
                positional += 1;
            }
            if let Some(a) = &p.arguments_object {
                header.shared.push(SharedEntry::Arguments {
                    local: p.id,
                    strict: a.strict,
                    mapped: a.mapped_parameter_ids.clone(),
                    restricted: a.restricted_callee,
                });
            }
        }
        self.packet_scope.replace(Builder {
            f,
            inert: self.packets_inert,
            packet: BodyPacket {
                header,
                ..Default::default()
            },
        })
    }

    pub(crate) fn packet_body(&mut self, f: FnIdx) -> Option<Builder> {
        if !self.packets_enabled {
            return None;
        }
        if self.funcs[f as usize].body_packet.ready() {
            return self.packet_scope.take();
        }
        let mut packet =
            Rc::unwrap_or_clone(std::mem::take(&mut self.funcs[f as usize].body_packet));
        packet.formals.clear();
        assert_eq!(packet.status, Status::Dormant, "a body is emitted once");
        if packet.header.identity.0.is_empty() {
            let fi = &self.funcs[f as usize];
            packet.header.identity = Identity(fi.module.to_string(), fi.func_id, fi.name.clone());
        }
        if packet.header.environment.is_none() {
            packet.header.environment = Some(Rc::new(Environment {
                owner: packet.header.identity.clone(),
                cells: Vec::new(),
                parent: None,
            }));
        }
        let fi = &self.funcs[f as usize];
        let ports = fi
            .params
            .iter()
            .enumerate()
            .map(|(i, n)| (Binding::Argument(i), *n))
            .chain([
                (Binding::Receiver, fi.this),
                (Binding::Result, fi.ret),
                (Binding::AllArguments, fi.args_all),
            ]);
        for (role, n) in ports {
            self.nodes[n as usize]
                .packet_interface
                .get_or_insert_with(|| (packet.header.identity.clone(), role));
        }
        let descriptor = Rc::make_mut(&mut self.funcs[f as usize].body_packet);
        descriptor.header = packet.header.clone();
        descriptor.status = Status::Building;
        packet.status = Status::Building;
        self.packet_scope.replace(Builder {
            f,
            inert: self.packets_inert,
            packet,
        })
    }

    pub(crate) fn packet_end(&mut self, prior: Option<Builder>, seal: bool) {
        if !self.packets_enabled {
            return;
        }
        let Some(builder) = self.packet_scope.take() else {
            self.packet_scope = prior;
            return;
        };
        let f = builder.f;
        let mut builder = builder;
        for birth in &mut builder.packet.births {
            if birth.kind.is_some() {
                continue;
            }
            let template = &self.tmpls[birth.template as usize];
            let (module, kind) = match &template.kind {
                TmplKind::Class {
                    module,
                    name,
                    class_id,
                    literal_shape,
                } => (
                    *module,
                    BirthKind::Class {
                        name: name.clone(),
                        id: *class_id,
                        literal: *literal_shape,
                    },
                ),
                TmplKind::Lit {
                    module, enum_image, ..
                } => (
                    *module,
                    BirthKind::Literal {
                        enum_image: *enum_image,
                    },
                ),
                TmplKind::FnInst { func } => {
                    let fi = &self.funcs[*func as usize];
                    let identity = if *func == f {
                        builder.packet.header.identity.clone()
                    } else {
                        fi.body_packet.header.identity.clone()
                    };
                    (fi.module, BirthKind::FunctionInstance(identity))
                }
                TmplKind::ObjCreate { module, .. } => (*module, BirthKind::ObjectCreate),
                TmplKind::Arr {
                    module, hole_free, ..
                } => (
                    *module,
                    BirthKind::Array {
                        hole_free: *hole_free,
                    },
                ),
                TmplKind::Args { module, .. } => (*module, BirthKind::Arguments),
                TmplKind::BObj { module, what, .. } => (*module, BirthKind::Builtin(what)),
            };
            birth.module = if module == self.funcs[f as usize].module {
                builder.packet.header.identity.0.clone()
            } else {
                module.to_string()
            };
            birth.kind = Some(kind);
            birth.keys = template
                .birth_keys
                .iter()
                .map(|a| self.atoms[*a as usize].clone())
                .collect();
        }
        if builder.packet.formals.is_empty() {
            let fi = &self.funcs[f as usize];
            let ports = fi
                .params
                .iter()
                .enumerate()
                .map(|(i, n)| (Binding::Argument(i), *n))
                .chain([
                    (Binding::Receiver, fi.this),
                    (Binding::Result, fi.ret),
                    (Binding::AllArguments, fi.args_all),
                ]);
            builder.packet.header.lexical_receiver = fi.lexical_this;
            for (binding, n) in ports {
                self.nodes[n as usize]
                    .packet_interface
                    .get_or_insert_with(|| {
                        (builder.packet.header.identity.clone(), binding.clone())
                    });
                let from_environment = match &binding {
                    Binding::Argument(i) => self
                        .capture_param(f, *i)
                        .map(|n| self.packet_term(&mut builder, n)),
                    _ => None,
                };
                let target = self.packet_term(&mut builder, n);
                builder.packet.formals.push(Formal {
                    binding,
                    target,
                    from_environment,
                });
            }
        }
        self.funcs[f as usize].body_packet = Rc::new(builder.finish(seal));
        self.packet_scope = prior;
    }

    pub(crate) fn packet_cell(&mut self, n: NodeId, local: u32) {
        let Some(scope) = &mut self.packet_scope else {
            return;
        };
        let cell = self.nodes[n as usize]
            .lexical_cell
            .get_or_insert_with(|| {
                scope
                    .packet
                    .header
                    .environment
                    .as_ref()
                    .and_then(|env| env.cell(local))
                    .unwrap_or_else(|| Cell {
                        owner: scope.packet.header.identity.clone(),
                        local,
                    })
            })
            .clone();
        if cell.owner == scope.packet.header.identity {
            scope.packet.cells.push(cell);
        }
    }

    pub(crate) fn packet_declarations(&mut self, body: &[perry_hir::Stmt]) {
        let Some(scope) = &mut self.packet_scope else {
            return;
        };
        let mut ids = scope
            .packet
            .header
            .environment
            .as_ref()
            .map(|e| e.cells.clone())
            .unwrap_or_default();
        perry_hir::tdz_check::for_each_stmt_shallow(body, &mut |s| {
            use perry_hir::Stmt;
            match s {
                Stmt::Let { id, .. } => ids.push(*id),
                Stmt::Try { catch: Some(c), .. } => {
                    if let Some((id, _)) = &c.param {
                        ids.push(*id);
                    }
                }
                Stmt::PreallocateBoxes(v) | Stmt::PreallocateTdzBoxes(v) => ids.extend(v),
                _ => {}
            }
        });
        ids.sort_unstable();
        ids.dedup();
        let environment = Rc::new(Environment {
            owner: scope.packet.header.identity.clone(),
            cells: ids,
            parent: scope
                .packet
                .header
                .environment
                .as_ref()
                .and_then(|e| e.parent.clone()),
        });
        scope.packet.cells = environment
            .cells
            .iter()
            .map(|id| Cell {
                owner: environment.owner.clone(),
                local: *id,
            })
            .collect();
        scope.packet.header.environment = Some(environment);
    }

    fn packet_term(&self, b: &mut Builder, n: NodeId) -> Term {
        let cell = self
            .nodes
            .get(n as usize)
            .and_then(|v| v.lexical_cell.clone());
        if let Some(origin) = self
            .nodes
            .get(n as usize)
            .and_then(|v| v.intrinsic_top.as_ref())
        {
            return Term::Intrinsic(origin.clone());
        }
        if cell.is_none() {
            if let Some((owner, role)) = self
                .nodes
                .get(n as usize)
                .and_then(|v| v.packet_interface.as_ref())
            {
                if owner != &b.packet.header.identity {
                    return Term::Interface(owner.clone(), role.clone());
                }
            }
        }
        if let Some(field) = self
            .nodes
            .get(n as usize)
            .and_then(|v| v.field_origin.as_ref())
        {
            let value = match field.object {
                Some(t) => self.packet_value(t, &b.packet),
                None => Value {
                    identity: "shared private cells".into(),
                    environment: None,
                    context: None,
                },
            };
            return Term::Field(value, field.key.clone());
        }
        let local = self
            .nodes
            .get(n as usize)
            .and_then(|v| v.packet_owner.as_ref())
            == Some(&b.packet.header.identity)
            || self
                .nodes
                .get(n as usize)
                .and_then(|v| v.packet_interface.as_ref())
                .is_some_and(|(owner, _)| owner == &b.packet.header.identity)
            || self.node_fn.get(n as usize) == Some(&b.f);
        b.term(n, cell, local)
    }

    fn packet_call(&self, b: &mut Builder, k: CallId) -> CallPort {
        let Some(c) = self.calls.get(k as usize) else {
            return CallPort {
                present: false,
                this: None,
                this_value: None,
                args: Vec::new(),
                spread: Vec::new(),
                ret: Term::Absent,
            };
        };
        CallPort {
            present: true,
            this: match c.this {
                ThisArg::Node(n) => Some(self.packet_term(b, n)),
                _ => None,
            },
            this_value: match c.this {
                ThisArg::Tok(t) => Some(self.packet_value(t, &b.packet)),
                _ => None,
            },
            args: c.args.iter().map(|n| self.packet_term(b, *n)).collect(),
            spread: c.spread.iter().map(|n| self.packet_term(b, *n)).collect(),
            ret: self.packet_term(b, c.ret),
        }
    }

    pub(crate) fn packet_field_protocol<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.packet_protocol_depth += 1;
        let result = f(self);
        self.packet_protocol_depth -= 1;
        result
    }

    pub(crate) fn packet_observe(
        &mut self,
        offset: u32,
        kind: String,
        key: String,
        receiver: NodeId,
        index: Option<NodeId>,
    ) {
        let Some(mut b) = self.packet_scope.take() else {
            return;
        };
        let receiver = self.packet_term(&mut b, receiver);
        let index = index.map(|n| self.packet_term(&mut b, n));
        b.packet.observations.push(Observation {
            offset,
            kind,
            key,
            receiver,
            index,
        });
        self.packet_scope = Some(b);
    }

    pub(super) fn packet_shape(&self, shape: KeyShapeId) -> Option<&[Vec<String>]> {
        (shape != 0).then(|| self.key_shapes[shape as usize].globs.as_slice())
    }

    pub(super) fn packet_key(&self, key: Key) -> String {
        match key {
            Key::Named(a) => format!("named {:?}", self.atoms[a as usize]),
            Key::Any(shape) => format!("computed {:?}", self.packet_shape(shape)),
        }
    }

    pub(crate) fn packet_generator_action(
        &mut self,
        f: FnIdx,
        object: NodeId,
        is_async: bool,
    ) -> Option<bool> {
        let mut b = self.packet_scope.take()?;
        let object = self.packet_term(&mut b, object);
        let function = self.funcs[f as usize].body_packet.header.identity.clone();
        b.packet.operations.push(Operation::GeneratorPrototype {
            function,
            object,
            is_async,
        });
        let publish = !b.inert;
        self.packet_scope = Some(b);
        self.packet_protocol_depth += 1;
        Some(publish)
    }

    pub(crate) fn packet_class_action(
        &mut self,
        module: &str,
        name: &str,
        action: ClassAction<NodeId>,
    ) -> Option<bool> {
        let mut b = self.packet_scope.take()?;
        let action = action.map(|n| self.packet_term(&mut b, n));
        b.packet.operations.push(Operation::ClassBinding {
            module: module.into(),
            name: name.into(),
            action,
        });
        let publish = !b.inert;
        self.packet_scope = Some(b);
        self.packet_protocol_depth += 1;
        Some(publish)
    }

    pub(crate) fn packet_shared_birth(&mut self, module: &str, id: u32, value: NodeId) {
        let Some(mut b) = self.packet_scope.take() else {
            return;
        };
        let term = self.packet_term(&mut b, value);
        b.packet.operations.push(Operation::SharedBirth(
            Identity(module.into(), id, String::new()),
            term,
        ));
        self.packet_scope = Some(b);
    }

    pub(crate) fn packet_direct_template(&self, node: NodeId) -> Option<TmplId> {
        self.packet_scope
            .as_ref()?
            .packet
            .emissions
            .iter()
            .rev()
            .find_map(|op| {
                let Emission::Seed(n, ts, _) = op else {
                    return None;
                };
                if *n != node {
                    return None;
                }
                ts.iter().find_map(|t| match self.toks[*t as usize] {
                    TokKind::Obj(tm, _) => Some(tm),
                    _ => None,
                })
            })
    }

    fn packet_value(&self, t: Tok, packet: &BodyPacket) -> Value {
        let environment = match self.toks[t as usize] {
            TokKind::Func(f, _) => match self.funcs.get(f as usize) {
                Some(fi) if fi.parent != NO_FN => fi
                    .body_packet
                    .header
                    .environment
                    .as_ref()
                    .and_then(|e| e.parent.clone())
                    .or_else(|| packet.header.environment.clone()),
                None => packet
                    .header
                    .environment
                    .as_ref()
                    .and_then(|e| e.parent.clone()),
                _ => None,
            },
            _ => None,
        };
        let context = if let Some(env) = &environment {
            if env.owner == packet.header.identity {
                Some(EnvironmentPort::Current)
            } else {
                let mut at = packet.header.environment.clone();
                let mut depth = 0;
                let mut port = EnvironmentPort::Shared;
                while let Some(e) = at {
                    if e.owner == env.owner {
                        port = EnvironmentPort::Outer(depth);
                        break;
                    }
                    depth += 1;
                    at = e.parent.clone();
                }
                Some(port)
            }
        } else if matches!(self.toks[t as usize], TokKind::Obj(tm,_) if packet.births.iter().any(|b| b.template==tm))
        {
            Some(EnvironmentPort::Current)
        } else {
            None
        };
        Value {
            identity: self.packet_token(t, packet),
            environment,
            context,
        }
    }

    pub(super) fn packet_token(&self, t: Tok, packet: &BodyPacket) -> String {
        match self.toks[t as usize] {
            TokKind::Func(f, ctx) => {
                let Some(fi) = self.funcs.get(f as usize) else {
                    return format!("function {:?} context {ctx}", packet.header.identity);
                };
                let identity = if !fi.body_packet.header.identity.0.is_empty() {
                    fi.body_packet.header.identity.clone()
                } else {
                    Identity(fi.module.to_string(), fi.func_id, fi.name.clone())
                };
                format!("function {identity:?} context {ctx}")
            }
            TokKind::Obj(tm, ctx) => match packet.births.iter().position(|v| v.template == tm) {
                Some(i) => format!("birth {i} context {ctx}"),
                None => format!("object {} context {ctx}", self.tmpls[tm as usize].display),
            },
            TokKind::ClassObj(tm) => format!("class {}", self.tmpls[tm as usize].display),
            TokKind::Proto(tm) => format!("prototype {}", self.tmpls[tm as usize].display),
            ref other => format!("{other:?}"),
        }
    }

    /// Interception precedes all live node effects. In mirror mode the
    /// existing publisher runs independently with interception suspended;
    /// in inert mode only this owned input grows.
    pub(crate) fn packet_emit(&mut self, event: Emission) -> bool {
        let Some(mut b) = self.packet_scope.take() else {
            return false;
        };
        let structural_seed = matches!(&event,Emission::Seed(n,_,_) if self.nodes.get(*n as usize).is_some_and(|v| v.intrinsic_top.is_some()));
        let op = if self.packet_protocol_depth > 0 || structural_seed {
            None
        } else {
            Some(match &event {
                Emission::Activate(f, path) => Operation::Activate(
                    self.funcs
                        .get(*f as usize)
                        .map(|fi| fi.body_packet.header.identity.clone()),
                    path.clone(),
                ),
                Emission::Escape(t, reason) => Operation::Escape(
                    self.packet_token(*t, &b.packet),
                    self.reasons[*reason as usize].clone(),
                ),
                Emission::ArrayMemberWrite(reason) => {
                    Operation::ArrayMemberWrite(self.reasons[*reason as usize].clone())
                }
                Emission::LoadFrom(origin, holder, key, dst) => Operation::LoadFrom(
                    self.packet_value(*origin, &b.packet),
                    self.packet_value(*holder, &b.packet),
                    self.packet_key(*key),
                    self.packet_term(&mut b, *dst),
                ),
                Emission::Seed(n, ts, p) => {
                    let target = self.packet_term(&mut b, *n);
                    let values = ts
                        .iter()
                        .map(|t| self.packet_value(*t, &b.packet))
                        .collect();
                    Operation::Seed(target, values, *p)
                }
                Emission::Edge(a, c) => {
                    Operation::Edge(self.packet_term(&mut b, *a), self.packet_term(&mut b, *c))
                }
                Emission::Rule(n, c) => {
                    let holder = self.packet_term(&mut b, *n);
                    let mut terms = Vec::new();
                    let mut calls = Vec::new();
                    let mut nodes = Vec::new();
                    let mut call_ids = Vec::new();
                    let c = crate::summary::map_cons(
                        c,
                        |n| {
                            let i = nodes.len() as u32;
                            nodes.push(n);
                            i
                        },
                        |k| {
                            let i = call_ids.len() as u32;
                            call_ids.push(k);
                            i
                        },
                    );
                    terms.extend(nodes.into_iter().map(|n| self.packet_term(&mut b, n)));
                    calls.extend(call_ids.into_iter().map(|k| self.packet_call(&mut b, k)));
                    Operation::Rule(holder, c, self.packet_rule(&c, &b.packet), terms, calls)
                }
            })
        };
        if let Some(op) = op {
            b.packet.operations.push(op);
        }
        b.packet.emissions.push(event.clone());
        if !b.inert {
            match event {
                Emission::Seed(n, ts, p) => self.add(n, &ts, p),
                Emission::Edge(a, c) => self.edge(a, c),
                Emission::Rule(n, c) => self.cons(n, c),
                Emission::LoadFrom(origin, holder, key, dst) => {
                    self.load_from(origin, holder, key, dst)
                }
                Emission::Activate(f, path) => self.activate(f, &path),
                Emission::Escape(t, reason) => self.escape(t, reason),
                Emission::ArrayMemberWrite(reason) => {
                    self.array_member_write.get_or_insert(reason);
                }
            }
        }
        self.packet_scope = Some(b);
        true
    }
}

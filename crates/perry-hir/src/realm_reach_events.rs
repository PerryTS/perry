//! Program events and declared observations, without unconditional kind roots.
use super::*;
use crate::realm_manifest::{
    RealmEffect, RealmKind, RealmOperand, RealmProtocol, RealmSlot, RealmSlotRoot, RealmSlotStep,
};

pub(super) fn primitive(expr: &ast::Expr) -> bool {
    match crate::patched_builtins::peel(expr) {
        ast::Expr::Lit(lit) => !matches!(lit, ast::Lit::Regex(_)),
        ast::Expr::Tpl(_) => true,
        ast::Expr::Unary(u) => matches!(
            u.op,
            ast::UnaryOp::Void
                | ast::UnaryOp::TypeOf
                | ast::UnaryOp::Bang
                | ast::UnaryOp::Plus
                | ast::UnaryOp::Minus
                | ast::UnaryOp::Tilde
        ),
        _ => false,
    }
}

impl Scan<'_> {
    pub(super) fn objects(&mut self) {
        if self.entries.has("Object.prototype") {
            self.out.chain_roots.insert("Object.prototype".into());
        }
    }
    pub(super) fn functions(&mut self) {
        if self.entries.has("Function.prototype") {
            self.out.chain_roots.insert("Function.prototype".into());
        }
    }
    pub(super) fn instance(&mut self, kind: &str) {
        self.add(kind);
        if let Some(proto) = self.entries.last(kind).and_then(|r| r.instance_prototype()) {
            self.out.chain_roots.insert(proto.to_string());
        }
    }

    pub(super) fn callable_value(&mut self, name: &str) {
        if self.entries.last(name).is_some_and(|r| {
            matches!(
                r.kind(),
                RealmKind::Constructor | RealmKind::Function | RealmKind::Method
            )
        }) {
            self.functions();
        }
    }
    pub(super) fn protocol(&mut self, expr: &ast::Expr, protocol: RealmProtocol) {
        if !primitive(expr) {
            self.out
                .receiver_keys
                .extend(protocol.receiver_keys().iter().map(|k| k.to_string()));
        }
    }

    /// Protocol operands are observed by their declared keys, not handed
    /// wholesale to unknown code. In particular, string coercion of a named
    /// namespace does not escape every callable it contains.
    pub(super) fn protocol_operand(&mut self, expr: &ast::Expr, protocol: RealmProtocol) {
        self.protocol(expr, protocol);
        if let Some(name) = self.holder(expr) {
            self.add(&name);
            if let Some(member) = member_view(expr) {
                self.member(member, true);
            }
        } else {
            expr.visit_with(self);
        }
    }

    fn slot_nonprimitive(
        &self,
        slot: &RealmSlot,
        callee: &ast::Expr,
        args: &[ast::ExprOrSpread],
    ) -> bool {
        if args.iter().any(|a| a.spread.is_some())
            && matches!(slot.root, RealmSlotRoot::Arg(_) | RealmSlotRoot::Rest(_))
        {
            return true;
        }
        let values: Vec<&ast::Expr> = match slot.root {
            RealmSlotRoot::This => member_view(callee)
                .map(|m| vec![&*m.obj])
                .unwrap_or_default(),
            RealmSlotRoot::Arg(i) => args
                .get(i as usize)
                .map(|a| vec![&*a.expr])
                .unwrap_or_default(),
            RealmSlotRoot::Rest(i) => args.iter().skip(i as usize).map(|a| &*a.expr).collect(),
            RealmSlotRoot::Callee => vec![callee],
            RealmSlotRoot::Result | RealmSlotRoot::Returned(_) => return true,
        };
        !values.is_empty() && (!slot.steps.is_empty() || values.iter().any(|v| !primitive(v)))
    }

    fn read_slot(&mut self, slot: &RealmSlot, callee: &ast::Expr, args: &[ast::ExprOrSpread]) {
        if !self.slot_nonprimitive(slot, callee, args) {
            return;
        }
        // Field steps declare KEY reads. They do not hand the loaded value
        // to unknown code, or inspect that value's [[Prototype]].
        let keyed = self.slot_key_reads(slot);
        if !keyed {
            self.out.chain_read = true;
        }
    }

    fn slot_key_reads(&mut self, slot: &RealmSlot) -> bool {
        let mut keyed = false;
        for step in slot.steps.iter() {
            if let RealmSlotStep::Field(key) = step {
                self.out.receiver_keys.insert(key.to_string());
                keyed = true;
            }
        }
        keyed
    }

    fn escape_operand(
        &mut self,
        operand: &RealmOperand,
        callee: &ast::Expr,
        args: &[ast::ExprOrSpread],
    ) {
        match operand {
            RealmOperand::Slot(slot) => self.escape_slot(slot, callee, args),
            RealmOperand::Fresh {
                elems,
                fields,
                slots,
                ..
            } => {
                for value in elems.iter() {
                    self.escape_operand(value, callee, args);
                }
                for field in fields.iter().chain(slots.iter()) {
                    self.escape_operand(&field.value, callee, args);
                }
            }
            RealmOperand::Promise { value, reject } => {
                for value in value.iter().chain(reject.iter()) {
                    self.escape_operand(value, callee, args);
                }
            }
            RealmOperand::Species {
                of,
                args: values,
                elems,
            } => {
                self.escape_slot(of, callee, args);
                for value in values.iter() {
                    self.escape_operand(value, callee, args);
                }
                for slot in elems.iter() {
                    self.escape_slot(slot, callee, args);
                }
            }
            RealmOperand::Union(values) => {
                for value in values.iter() {
                    self.escape_operand(value, callee, args);
                }
            }
            _ => {}
        }
    }

    fn escape_slot(&mut self, slot: &RealmSlot, callee: &ast::Expr, args: &[ast::ExprOrSpread]) {
        if self.slot_nonprimitive(slot, callee, args) {
            self.out.chain_read = true;
            self.out.any_receiver_key = true;
        }
    }

    fn unknown_code_inputs(&mut self, callee: &ast::Expr, args: &[ast::ExprOrSpread]) {
        if member_view(callee).is_some_and(|m| !primitive(&m.obj))
            || args
                .iter()
                .any(|a| a.spread.is_some() || !primitive(&a.expr))
        {
            self.out.chain_read = true;
            self.out.any_receiver_key = true;
        }
    }

    pub(super) fn call_events(&mut self, callee: &ast::Expr, args: &[ast::ExprOrSpread]) {
        let name = self.holder(callee).or_else(|| {
            let member = member_view(callee)?;
            let kind = match crate::patched_builtins::peel(&member.obj) {
                ast::Expr::Lit(ast::Lit::Str(_)) | ast::Expr::Tpl(_) => "String",
                ast::Expr::Lit(ast::Lit::Num(_)) => "Number",
                ast::Expr::Lit(ast::Lit::Bool(_)) => "Boolean",
                ast::Expr::Lit(ast::Lit::BigInt(_)) => "BigInt",
                ast::Expr::Lit(ast::Lit::Regex(_)) => "RegExp",
                ast::Expr::Array(_) => "Array",
                _ => return None,
            };
            let key = self.key(&member.prop)?;
            self.entries
                .prototype_members(&[kind], Some(&key))
                .first()
                .map(|n| n.to_string())
        });
        let row = name.as_deref().and_then(|n| self.entries.last(n));
        if row.is_none() {
            self.primitive_prototypes();
        }
        if let Some(body) = row.and_then(|r| r.operation().or_else(|| r.body())) {
            let symbol = body.rsplit("::").next().unwrap();
            if self.entries.operation(symbol).is_some() {
                self.out.exec(symbol);
            }
        }
        let effects = row.map(|r| r.entry().effects).unwrap_or(&[]);
        if effects.is_empty() && !row.is_some_and(|r| r.argument_mode("converted")) {
            self.unknown_code_inputs(callee, args);
        }
        for effect in effects {
            match effect {
                RealmEffect::Reads(slots) => {
                    for slot in slots.iter() {
                        self.read_slot(slot, callee, args);
                    }
                }
                RealmEffect::Coerce { slot, protocol } => {
                    if self.slot_nonprimitive(slot, callee, args) {
                        self.slot_key_reads(slot);
                        self.out
                            .receiver_keys
                            .extend(protocol.receiver_keys().iter().map(|k| k.to_string()));
                    }
                }
                RealmEffect::Escapes(slot) | RealmEffect::AsyncResource(slot) => {
                    self.escape_slot(slot, callee, args)
                }
                RealmEffect::Calls {
                    callee: slot,
                    this,
                    args: operands,
                } => {
                    self.read_slot(slot, callee, args);
                    for operand in std::iter::once(this).chain(operands.iter()) {
                        self.escape_operand(operand, callee, args);
                    }
                }
                RealmEffect::Open => self.unknown_code_inputs(callee, args),
                _ => {}
            }
        }
    }
}

//! Class definition evaluation: computed member names, static initializers and
//! static blocks, for a class declared once (template-keyed nodes) and for each
//! evaluation of a class in a function body (`Expr::ClassExprFresh`, bracketed by
//! `class_eval_begin` / `class_eval_end`; see `__classDefining` in wasm_runtime.js).

use super::*;

/// `RegisterClassComputed*` kinds understood by `class_define_computed`.
const COMPUTED_METHOD: f64 = 0.0;
const COMPUTED_ACCESSOR: f64 = 1.0;

impl<'a> FuncEmitCtx<'a> {
    pub(super) fn try_emit_expr_class_eval(&mut self, func: &mut Function, expr: &Expr) -> bool {
        match expr {
            // Every evaluation of a class in a function body is fresh, the first one
            // included, so no evaluation is the shared class.
            Expr::ClassIsFirstEvaluation { value, .. } => {
                self.with_tdz_suppressed(|ctx| ctx.emit_expr(func, value));
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
            }
            Expr::ClassExprFresh {
                template,
                evaluation_owner,
                named_statics,
                computed_keys,
                computed_statics,
                static_init_order,
                captured_args,
                definition_steps,
                ..
            } => {
                // ClassDefinitionEvaluation order: the binding, the computed names in
                // source order, then static fields and blocks in source order.
                let ctor_slot = self.class_ctor_slot(template);
                self.emit_class_bridge(
                    func,
                    "class_eval_begin",
                    template,
                    &[&Expr::Number(ctor_slot)],
                );
                match evaluation_owner {
                    Some(owner) => {
                        self.emit_local_tee(func, owner);
                        func.instruction(&Instruction::Drop);
                    }
                    None => {
                        func.instruction(&Instruction::Drop);
                    }
                }
                for step in definition_steps {
                    self.emit_expr(func, step);
                    func.instruction(&Instruction::Drop);
                }
                for (slot, key) in computed_keys {
                    let key = template_static_read(template, key);
                    self.emit_class_bridge(
                        func,
                        "class_eval_slot",
                        template,
                        &[&Expr::String(slot.clone()), &key],
                    );
                    func.instruction(&Instruction::Drop);
                }
                if !captured_args.is_empty() {
                    let caps = Expr::Array(captured_args.clone(), None);
                    self.with_tdz_suppressed(|ctx| {
                        ctx.emit_class_bridge(func, "class_eval_caps", template, &[&caps])
                    });
                    func.instruction(&Instruction::Drop);
                }
                for step in static_init_order {
                    match step {
                        perry_hir::ClassFreshStaticInit::Named(i) => {
                            let Some((name, init)) = named_statics.get(*i as usize) else {
                                continue;
                            };
                            self.emit_class_bridge(
                                func,
                                "class_eval_static",
                                template,
                                &[&Expr::String(name.clone()), init],
                            );
                        }
                        perry_hir::ClassFreshStaticInit::Computed(i) => {
                            let Some((slot, init)) = computed_statics.get(*i as usize) else {
                                continue;
                            };
                            self.emit_class_bridge(
                                func,
                                "class_eval_static_computed",
                                template,
                                &[&Expr::String(slot.clone()), init],
                            );
                        }
                        perry_hir::ClassFreshStaticInit::Block(i) => {
                            let block = self.static_block_slot(template, *i);
                            self.emit_class_bridge(
                                func,
                                "class_eval_block",
                                template,
                                &[&Expr::Number(block)],
                            );
                        }
                    }
                    func.instruction(&Instruction::Drop);
                }
                self.emit_class_bridge(func, "class_eval_end", template, &[]);
            }
            Expr::RegisterClassComputedMethod {
                class_name,
                key_expr,
                method_name,
                is_static,
                ..
            } => {
                let method = self.computed_fn_slot(class_name, Some(method_name));
                let proto_kind = self
                    .emitter
                    .class_computed_map
                    .get(&(class_name.clone(), method_name.clone()))
                    .map_or(0.0, |&idx| self.emitter.fn_proto_code(idx));
                self.emit_define_computed(
                    func,
                    class_name,
                    key_expr,
                    (COMPUTED_METHOD, proto_kind),
                    *is_static,
                    [method, -1.0],
                );
            }
            Expr::RegisterClassComputedAccessor {
                class_name,
                key_expr,
                getter_name,
                setter_name,
                is_static,
                ..
            } => {
                let getter = self.computed_fn_slot(class_name, getter_name.as_ref());
                let setter = self.computed_fn_slot(class_name, setter_name.as_ref());
                self.emit_define_computed(
                    func,
                    class_name,
                    key_expr,
                    (COMPUTED_ACCESSOR, 0.0),
                    *is_static,
                    [getter, setter],
                );
            }
            // `static [key] = value` of a class declared once.
            Expr::ClassStaticSymbolSet {
                class_name,
                key,
                value,
            } => {
                let ctor_slot = self.class_ctor_slot(class_name);
                let key = template_static_read(class_name, key);
                self.emit_class_bridge(
                    func,
                    "class_static_define",
                    class_name,
                    &[&Expr::Number(ctor_slot), &key, value],
                );
            }
            _ => return false,
        }
        true
    }

    /// `class_define_computed(class, ctorSlot, key, kind, isStatic, a, b, protoKind)`,
    /// `protoKind` being a method's `fn_proto_code`.
    fn emit_define_computed(
        &mut self,
        func: &mut Function,
        class_name: &str,
        key_expr: &Expr,
        (kind, proto_kind): (f64, f64),
        is_static: bool,
        slots: [f64; 2],
    ) {
        let ctor_slot = self.class_ctor_slot(class_name);
        self.emit_class_bridge(
            func,
            "class_define_computed",
            class_name,
            &[
                &Expr::Number(ctor_slot),
                key_expr,
                &Expr::Number(kind),
                &Expr::Bool(is_static),
                &Expr::Number(slots[0]),
                &Expr::Number(slots[1]),
                &Expr::Number(proto_kind),
            ],
        );
    }

    /// The table slot of `class_name`'s computed member function `name`, or -1.
    fn computed_fn_slot(&self, class_name: &str, name: Option<&String>) -> f64 {
        name.and_then(|name| {
            self.emitter
                .class_computed_map
                .get(&(class_name.to_string(), name.clone()))
        })
        .and_then(|idx| self.emitter.func_to_table_idx.get(idx))
        .map_or(-1.0, |&slot| slot as f64)
    }

    /// The table slot of `class_name`'s static block `index`, or -1.
    fn static_block_slot(&self, class_name: &str, index: u32) -> f64 {
        self.emitter
            .class_static_map
            .get(class_name)
            .and_then(|statics| statics.get(&format!("__perry_static_init_{index}")))
            .and_then(|idx| self.emitter.func_to_table_idx.get(idx))
            .map_or(-1.0, |&slot| slot as f64)
    }
}

/// A computed key slot is written on the class's static side
/// (`StaticFieldSet { class, slot }`) and read back as
/// `PropertyGet(ClassRef(class), slot)`; the read takes the same static storage.
fn template_static_read(class_name: &str, key: &Expr) -> Expr {
    match key {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(object.as_ref(), Expr::ClassRef(c) if c == class_name) => {
            Expr::StaticFieldGet {
                class_name: class_name.to_string(),
                field_name: property.clone(),
            }
        }
        _ => key.clone(),
    }
}

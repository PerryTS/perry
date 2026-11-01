//! `Expr::clone` without recursion per nesting level.
//!
//! A derived clone of `Expr` recursed once per sub-expression, with the
//! frame of a match over every variant at each level (15.7 KB in a debug
//! build), so copying a left-deep chain such as `o.k0 + … + o.k63` took a
//! megabyte of stack. Whole bodies are copied for real: the run-once view of
//! per-unit facts (`perry_wpfacts::decl_units::flatten`) is a module beside
//! the one codegen emits, and lowering and codegen copy closure literals.
//!
//! This clone walks the tree in post order with explicit stacks. Each node
//! is copied once by [`shallow`], which copies its own fields and leaves a
//! hole (`Expr::Undefined`) at every direct sub-expression the walkers visit,
//! and the holes take the copies of its children in the walkers' order
//! ([`walk_expr_children`] to list them, [`walk_expr_children_mut`] to fill
//! them: the two visit the same fields in the same order). A closure's body
//! is a statement list, which the walkers do not enter: `shallow` copies it
//! with `Stmt::clone`, whose expressions come back here, so the stack grows
//! per nested body, not per operand.
//!
//! `shallow` has an arm per variant (no catch-all: a new variant is a
//! compile error here, as in the walkers). A sub-expression field (`Expr`,
//! `Box<Expr>`, `Option<Box<Expr>>`, the lists of them, the expressions of
//! `CallArg`, `ArrayElement` and a parameter's default) is a hole; every
//! other field is cloned. Every copy checks that it made one hole per child
//! the walkers visit and that each visited field is a hole.

use super::*;

impl Clone for Expr {
    fn clone(&self) -> Expr {
        let mut kids = 0;
        walk_expr_children(self, &mut |_| kids += 1);
        if kids == 0 {
            return copy_node(self, 0, &mut Vec::new());
        }
        // Post order is the reverse of a pre order that takes the children
        // last to first; `order` holds each node with its number of children.
        let mut todo: Vec<&Expr> = vec![self];
        let mut order: Vec<(&Expr, usize)> = Vec::new();
        while let Some(e) = todo.pop() {
            let before = todo.len();
            walk_expr_children(e, &mut |c| todo.push(c));
            order.push((e, todo.len() - before));
        }
        // The copies of the nodes done whose parent is not yet: a node's
        // children are the last ones, first to last.
        let mut done: Vec<Expr> = Vec::new();
        for &(e, kids) in order.iter().rev() {
            let copy = copy_node(e, kids, &mut done);
            done.push(copy);
        }
        debug_assert_eq!(done.len(), 1);
        done.pop().expect("the copy of the root")
    }
}

/// The copy of `e`, its `kids` children's copies the last ones of `done`
/// (taken from it).
fn copy_node(e: &Expr, kids: usize, done: &mut Vec<Expr>) -> Expr {
    let mut holes = 0;
    let mut copy = shallow(e, &mut holes);
    assert_eq!(
        holes, kids,
        "internal compiler error: Expr::clone: the shallow copy has {holes} holes, the walkers visit {kids} children"
    );
    if kids > 0 {
        let mut copies = done.drain(done.len() - kids..);
        walk_expr_children_mut(&mut copy, &mut |slot| {
            assert!(
                matches!(slot, Expr::Undefined),
                "internal compiler error: Expr::clone: a child the walkers visit is no hole of the shallow copy"
            );
            *slot = copies.next().expect("a copy per child");
        });
    }
    copy
}

/// A field of a node in its shallow copy: a hole for each sub-expression
/// (counted in `n`), the rest cloned.
trait Shallow {
    fn shallow(&self, n: &mut usize) -> Self;
}

fn hole(n: &mut usize) -> Expr {
    *n += 1;
    Expr::Undefined
}

impl Shallow for Expr {
    fn shallow(&self, n: &mut usize) -> Expr {
        hole(n)
    }
}

impl Shallow for Box<Expr> {
    fn shallow(&self, n: &mut usize) -> Box<Expr> {
        Box::new(hole(n))
    }
}

impl Shallow for Option<Box<Expr>> {
    fn shallow(&self, n: &mut usize) -> Option<Box<Expr>> {
        self.as_ref().map(|_| Box::new(hole(n)))
    }
}

impl Shallow for Vec<Expr> {
    fn shallow(&self, n: &mut usize) -> Vec<Expr> {
        self.iter().map(|_| hole(n)).collect()
    }
}

impl<K: Clone> Shallow for Vec<(K, Expr)> {
    fn shallow(&self, n: &mut usize) -> Vec<(K, Expr)> {
        self.iter().map(|(k, _)| (k.clone(), hole(n))).collect()
    }
}

impl<K: Clone> Shallow for Vec<(K, Box<Expr>)> {
    fn shallow(&self, n: &mut usize) -> Vec<(K, Box<Expr>)> {
        self.iter()
            .map(|(k, _)| (k.clone(), Box::new(hole(n))))
            .collect()
    }
}

impl Shallow for Vec<CallArg> {
    fn shallow(&self, n: &mut usize) -> Vec<CallArg> {
        self.iter()
            .map(|a| match a {
                CallArg::Expr(_) => CallArg::Expr(hole(n)),
                CallArg::Spread(_) => CallArg::Spread(hole(n)),
            })
            .collect()
    }
}

impl Shallow for Vec<ArrayElement> {
    fn shallow(&self, n: &mut usize) -> Vec<ArrayElement> {
        self.iter()
            .map(|a| match a {
                ArrayElement::Expr(_) => ArrayElement::Expr(hole(n)),
                ArrayElement::Hole => ArrayElement::Hole,
                ArrayElement::Spread(_) => ArrayElement::Spread(hole(n)),
            })
            .collect()
    }
}

/// A closure's parameters: the walkers visit each default.
impl Shallow for Vec<Param> {
    fn shallow(&self, n: &mut usize) -> Vec<Param> {
        self.iter()
            .map(|p| {
                let Param {
                    id,
                    name,
                    ty,
                    default,
                    decorators,
                    is_rest,
                    arguments_object,
                } = p;
                Param {
                    id: *id,
                    name: name.clone(),
                    ty: ty.clone(),
                    default: default.as_ref().map(|_| hole(n)),
                    decorators: decorators.clone(),
                    is_rest: *is_rest,
                    arguments_object: arguments_object.clone(),
                }
            })
            .collect()
    }
}

/// The copy of `e` with a hole (counted in `n`) at each sub-expression.
#[allow(clippy::clone_on_copy)]
fn shallow(e: &Expr, n: &mut usize) -> Expr {
    match e {
        Expr::Undefined => Expr::Undefined,
        Expr::Null => Expr::Null,
        Expr::Bool(a0) => Expr::Bool(a0.clone()),
        Expr::Number(a0) => Expr::Number(a0.clone()),
        Expr::Integer(a0) => Expr::Integer(a0.clone()),
        Expr::BigInt(a0) => Expr::BigInt(a0.clone()),
        Expr::String(a0) => Expr::String(a0.clone()),
        Expr::WtfString(a0) => Expr::WtfString(a0.clone()),
        Expr::I18nString {
            key,
            string_idx,
            params,
            plural_forms,
            plural_param,
        } => Expr::I18nString {
            key: key.clone(),
            string_idx: string_idx.clone(),
            params: Shallow::shallow(params, n),
            plural_forms: plural_forms.clone(),
            plural_param: plural_param.clone(),
        },
        Expr::LocalGet(a0) => Expr::LocalGet(a0.clone()),
        Expr::LocalSet(a0, a1) => Expr::LocalSet(a0.clone(), Shallow::shallow(a1, n)),
        Expr::ScopedTemp { id, value, body } => Expr::ScopedTemp {
            id: id.clone(),
            value: Shallow::shallow(value, n),
            body: Shallow::shallow(body, n),
        },
        Expr::GlobalGet(a0) => Expr::GlobalGet(a0.clone()),
        Expr::GlobalSet(a0, a1) => Expr::GlobalSet(a0.clone(), Shallow::shallow(a1, n)),
        Expr::WithGet {
            object,
            property,
            fallback,
        } => Expr::WithGet {
            object: Shallow::shallow(object, n),
            property: property.clone(),
            fallback: Shallow::shallow(fallback, n),
        },
        Expr::WithSet {
            object,
            property,
            value,
            fallback,
            strict,
        } => Expr::WithSet {
            object: Shallow::shallow(object, n),
            property: property.clone(),
            value: Shallow::shallow(value, n),
            fallback: fallback.clone(),
            strict: strict.clone(),
        },
        Expr::Update { id, op, prefix } => Expr::Update {
            id: id.clone(),
            op: op.clone(),
            prefix: prefix.clone(),
        },
        Expr::Binary { op, left, right } => Expr::Binary {
            op: op.clone(),
            left: Shallow::shallow(left, n),
            right: Shallow::shallow(right, n),
        },
        Expr::Unary { op, operand } => Expr::Unary {
            op: op.clone(),
            operand: Shallow::shallow(operand, n),
        },
        Expr::Compare { op, left, right } => Expr::Compare {
            op: op.clone(),
            left: Shallow::shallow(left, n),
            right: Shallow::shallow(right, n),
        },
        Expr::Logical { op, left, right } => Expr::Logical {
            op: op.clone(),
            left: Shallow::shallow(left, n),
            right: Shallow::shallow(right, n),
        },
        Expr::Call {
            callee,
            args,
            type_args,
            byte_offset,
            byte_end,
        } => Expr::Call {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
            type_args: type_args.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::CallSpread {
            callee,
            args,
            type_args,
            byte_offset,
            byte_end,
        } => Expr::CallSpread {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
            type_args: type_args.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::SuperCallSpread(a0) => Expr::SuperCallSpread(Shallow::shallow(a0, n)),
        Expr::FuncRef(a0) => Expr::FuncRef(a0.clone()),
        Expr::ExternFuncRef {
            name,
            param_types,
            return_type,
        } => Expr::ExternFuncRef {
            name: name.clone(),
            param_types: param_types.clone(),
            return_type: return_type.clone(),
        },
        Expr::NativeModuleRef(a0) => Expr::NativeModuleRef(a0.clone()),
        Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method,
            args,
        } => Expr::NativeMethodCall {
            module: module.clone(),
            class_name: class_name.clone(),
            object: Shallow::shallow(object, n),
            method: method.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            byte_end,
        } => Expr::PropertyGet {
            object: Shallow::shallow(object, n),
            property: property.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::PropertySet {
            object,
            property,
            value,
            byte_offset,
            byte_end,
        } => Expr::PropertySet {
            object: Shallow::shallow(object, n),
            property: property.clone(),
            value: Shallow::shallow(value, n),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::PropertyUpdate {
            object,
            property,
            op,
            prefix,
            strict,
            byte_offset,
            byte_end,
        } => Expr::PropertyUpdate {
            object: Shallow::shallow(object, n),
            property: property.clone(),
            op: op.clone(),
            prefix: prefix.clone(),
            strict: strict.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::IndexGet {
            object,
            index,
            byte_offset,
            byte_end,
        } => Expr::IndexGet {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::IndexSet {
            object,
            index,
            value,
            byte_offset,
            byte_end,
        } => Expr::IndexSet {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::IndexUpdate {
            object,
            index,
            op,
            prefix,
            strict,
            byte_offset,
            byte_end,
        } => Expr::IndexUpdate {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            op: op.clone(),
            prefix: prefix.clone(),
            strict: strict.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::Object(a0) => Expr::Object(Shallow::shallow(a0, n)),
        Expr::LiteralChunks(a0) => Expr::LiteralChunks(Shallow::shallow(a0, n)),
        Expr::LiteralChunkObject => Expr::LiteralChunkObject,
        Expr::LiteralSlotInit {
            object,
            key,
            slot,
            value,
        } => Expr::LiteralSlotInit {
            object: Shallow::shallow(object, n),
            key: key.clone(),
            slot: slot.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::ObjectSpread { parts } => Expr::ObjectSpread {
            parts: Shallow::shallow(parts, n),
        },
        Expr::ObjectAssign { target, sources } => Expr::ObjectAssign {
            target: Shallow::shallow(target, n),
            sources: Shallow::shallow(sources, n),
        },
        Expr::Array(a0, a1) => Expr::Array(Shallow::shallow(a0, n), a1.clone()),
        Expr::ArraySpread(a0) => Expr::ArraySpread(Shallow::shallow(a0, n)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => Expr::Conditional {
            condition: Shallow::shallow(condition, n),
            then_expr: Shallow::shallow(then_expr, n),
            else_expr: Shallow::shallow(else_expr, n),
        },
        Expr::TypeOf(a0) => Expr::TypeOf(Shallow::shallow(a0, n)),
        Expr::Void(a0) => Expr::Void(Shallow::shallow(a0, n)),
        Expr::InstanceOf { expr, ty, ty_expr } => Expr::InstanceOf {
            expr: Shallow::shallow(expr, n),
            ty: ty.clone(),
            ty_expr: Shallow::shallow(ty_expr, n),
        },
        Expr::In { property, object } => Expr::In {
            property: Shallow::shallow(property, n),
            object: Shallow::shallow(object, n),
        },
        Expr::PrivateLexicalBrand(a0) => Expr::PrivateLexicalBrand(Shallow::shallow(a0, n)),
        Expr::PrivateLexicalBrandPush(a0) => Expr::PrivateLexicalBrandPush(Shallow::shallow(a0, n)),
        Expr::PrivateLexicalBrandPop => Expr::PrivateLexicalBrandPop,
        Expr::PrivateBrandCheck {
            class_name,
            class_id,
            field_name,
            kind,
            is_static,
            receiver_is_brand_owner,
            object,
        } => Expr::PrivateBrandCheck {
            class_name: class_name.clone(),
            class_id: class_id.clone(),
            field_name: field_name.clone(),
            kind: kind.clone(),
            is_static: is_static.clone(),
            receiver_is_brand_owner: receiver_is_brand_owner.clone(),
            object: Shallow::shallow(object, n),
        },
        Expr::PrivateGuard {
            class_name,
            class_id,
            field_name,
            kind,
            op,
            receiver_is_brand_owner,
            object,
        } => Expr::PrivateGuard {
            class_name: class_name.clone(),
            class_id: class_id.clone(),
            field_name: field_name.clone(),
            kind: kind.clone(),
            op: op.clone(),
            receiver_is_brand_owner: receiver_is_brand_owner.clone(),
            object: Shallow::shallow(object, n),
        },
        Expr::Await(a0) => Expr::Await(Shallow::shallow(a0, n)),
        Expr::Yield { value, delegate } => Expr::Yield {
            value: Shallow::shallow(value, n),
            delegate: delegate.clone(),
        },
        Expr::New {
            class_name,
            args,
            type_args,
            byte_offset,
            cap_args_appended,
            byte_end,
        } => Expr::New {
            class_name: class_name.clone(),
            args: Shallow::shallow(args, n),
            type_args: type_args.clone(),
            byte_offset: byte_offset.clone(),
            cap_args_appended: cap_args_appended.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::NewDynamic {
            callee,
            args,
            byte_offset,
            byte_end,
        } => Expr::NewDynamic {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::NewDynamicSpread {
            callee,
            args,
            byte_offset,
            byte_end,
        } => Expr::NewDynamicSpread {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::NewTarget => Expr::NewTarget,
        Expr::ClassRef(a0) => Expr::ClassRef(a0.clone()),
        Expr::ClassMethodHome {
            class_name,
            is_static,
        } => Expr::ClassMethodHome {
            class_name: class_name.clone(),
            is_static: is_static.clone(),
        },
        Expr::EnumMember {
            enum_name,
            member_name,
        } => Expr::EnumMember {
            enum_name: enum_name.clone(),
            member_name: member_name.clone(),
        },
        Expr::EnumImage {
            class_name,
            stores,
            strict,
        } => Expr::EnumImage {
            class_name: class_name.clone(),
            stores: stores.clone(),
            strict: strict.clone(),
        },
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => Expr::StaticFieldGet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
        },
        Expr::StaticFieldSet {
            class_name,
            field_name,
            value,
        } => Expr::StaticFieldSet {
            class_name: class_name.clone(),
            field_name: field_name.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::ClassStaticSymbolSet {
            class_name,
            key,
            value,
        } => Expr::ClassStaticSymbolSet {
            class_name: class_name.clone(),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
        },
        Expr::RegisterClassParentDynamic {
            class_name,
            parent_expr,
        } => Expr::RegisterClassParentDynamic {
            class_name: class_name.clone(),
            parent_expr: Shallow::shallow(parent_expr, n),
        },
        Expr::RegisterClassCaptures {
            class_name,
            captures,
        } => Expr::RegisterClassCaptures {
            class_name: class_name.clone(),
            captures: Shallow::shallow(captures, n),
        },
        Expr::RefreshClassExprCaptures {
            class_value,
            captures,
            env_class,
        } => Expr::RefreshClassExprCaptures {
            class_value: Shallow::shallow(class_value, n),
            captures: Shallow::shallow(captures, n),
            env_class: env_class.clone(),
        },
        Expr::ClassEnvGet {
            class_name,
            index,
            guarded,
        } => Expr::ClassEnvGet {
            class_name: class_name.clone(),
            index: index.clone(),
            guarded: guarded.clone(),
        },
        Expr::ClassEnvSet {
            class_name,
            index,
            value,
            guarded,
            publish,
        } => Expr::ClassEnvSet {
            class_name: class_name.clone(),
            index: index.clone(),
            value: Shallow::shallow(value, n),
            guarded: guarded.clone(),
            publish: publish.clone(),
        },
        Expr::ClassEnvStamp {
            class_name,
            instance,
            evaluation,
        } => Expr::ClassEnvStamp {
            class_name: class_name.clone(),
            instance: Shallow::shallow(instance, n),
            evaluation: Shallow::shallow(evaluation, n),
        },
        Expr::ClassEnvCurrent { class_name } => Expr::ClassEnvCurrent {
            class_name: class_name.clone(),
        },
        Expr::ClassCaptureValue {
            class_name,
            index,
            fallback,
            prefer_fallback,
        } => Expr::ClassCaptureValue {
            class_name: class_name.clone(),
            index: index.clone(),
            fallback: Shallow::shallow(fallback, n),
            prefer_fallback: prefer_fallback.clone(),
        },
        Expr::RegisterClassStaticSymbol {
            class_name,
            key_expr,
            value_expr,
        } => Expr::RegisterClassStaticSymbol {
            class_name: class_name.clone(),
            key_expr: Shallow::shallow(key_expr, n),
            value_expr: Shallow::shallow(value_expr, n),
        },
        Expr::RegisterClassComputedMethod {
            class_name,
            key_expr,
            method_name,
            is_static,
            param_count,
            has_rest,
            definition_order,
        } => Expr::RegisterClassComputedMethod {
            class_name: class_name.clone(),
            key_expr: Shallow::shallow(key_expr, n),
            method_name: method_name.clone(),
            is_static: is_static.clone(),
            param_count: param_count.clone(),
            has_rest: has_rest.clone(),
            definition_order: definition_order.clone(),
        },
        Expr::RegisterClassComputedAccessor {
            class_name,
            key_expr,
            getter_name,
            setter_name,
            is_static,
            definition_order,
        } => Expr::RegisterClassComputedAccessor {
            class_name: class_name.clone(),
            key_expr: Shallow::shallow(key_expr, n),
            getter_name: getter_name.clone(),
            setter_name: setter_name.clone(),
            is_static: is_static.clone(),
            definition_order: definition_order.clone(),
        },
        Expr::ClassExprFresh {
            template,
            evaluation_owner,
            named_statics,
            computed_keys,
            computed_statics,
            static_init_order,
            captured_args,
            shared_first_evaluation,
            evaluated_parent,
            definition_steps,
        } => Expr::ClassExprFresh {
            template: template.clone(),
            evaluation_owner: evaluation_owner.clone(),
            named_statics: Shallow::shallow(named_statics, n),
            computed_keys: Shallow::shallow(computed_keys, n),
            computed_statics: Shallow::shallow(computed_statics, n),
            static_init_order: static_init_order.clone(),
            captured_args: Shallow::shallow(captured_args, n),
            shared_first_evaluation: Shallow::shallow(shared_first_evaluation, n),
            evaluated_parent: Shallow::shallow(evaluated_parent, n),
            definition_steps: Shallow::shallow(definition_steps, n),
        },
        Expr::ClassIsFirstEvaluation { value, template } => Expr::ClassIsFirstEvaluation {
            value: Shallow::shallow(value, n),
            template: template.clone(),
        },
        Expr::SetFunctionPrototype {
            func,
            proto,
            strict,
        } => Expr::SetFunctionPrototype {
            func: Shallow::shallow(func, n),
            proto: Shallow::shallow(proto, n),
            strict: strict.clone(),
        },
        Expr::RegisterPrototypeMethod {
            class_name,
            method_name,
            value,
        } => Expr::RegisterPrototypeMethod {
            class_name: class_name.clone(),
            method_name: method_name.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::RegisterFunctionPrototypeMethod {
            func,
            method_name,
            value,
        } => Expr::RegisterFunctionPrototypeMethod {
            func: Shallow::shallow(func, n),
            method_name: method_name.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::GetFunctionPrototypeMethod { func, method_name } => {
            Expr::GetFunctionPrototypeMethod {
                func: Shallow::shallow(func, n),
                method_name: method_name.clone(),
            }
        }
        Expr::StaticMethodCall {
            class_name,
            method_name,
            args,
        } => Expr::StaticMethodCall {
            class_name: class_name.clone(),
            method_name: method_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::This => Expr::This,
        Expr::SuperCall(a0) => Expr::SuperCall(Shallow::shallow(a0, n)),
        Expr::SuperMethodCall { method, args } => Expr::SuperMethodCall {
            method: method.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::SuperMethodCallSpread { method, args } => Expr::SuperMethodCallSpread {
            method: method.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::SuperPropertyGet { property } => Expr::SuperPropertyGet {
            property: property.clone(),
        },
        Expr::SuperPropertySet {
            parent_class_id,
            parent_class_name,
            key,
            value,
        } => Expr::SuperPropertySet {
            parent_class_id: parent_class_id.clone(),
            parent_class_name: parent_class_name.clone(),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
        },
        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => Expr::ObjectSuperPropertyGet {
            home: Shallow::shallow(home, n),
            key: Shallow::shallow(key, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => Expr::ObjectSuperPropertySet {
            home: Shallow::shallow(home, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => Expr::ObjectSuperMethodCall {
            home: Shallow::shallow(home, n),
            key: Shallow::shallow(key, n),
            receiver: Shallow::shallow(receiver, n),
            args: Shallow::shallow(args, n),
        },
        Expr::EnvGet(a0) => Expr::EnvGet(a0.clone()),
        Expr::EnvGetDynamic(a0) => Expr::EnvGetDynamic(Shallow::shallow(a0, n)),
        Expr::ProcessEnv => Expr::ProcessEnv,
        Expr::GlobalThisExpr => Expr::GlobalThisExpr,
        Expr::NamespacePatched(a0) => Expr::NamespacePatched(a0.clone()),
        Expr::ModuleTopThis => Expr::ModuleTopThis,
        Expr::ProcessUptime => Expr::ProcessUptime,
        Expr::ProcessCwd => Expr::ProcessCwd,
        Expr::ProcessArgv => Expr::ProcessArgv,
        Expr::ProcessMemoryUsage => Expr::ProcessMemoryUsage,
        Expr::ProcessPid => Expr::ProcessPid,
        Expr::ProcessPpid => Expr::ProcessPpid,
        Expr::ProcessVersion => Expr::ProcessVersion,
        Expr::ProcessVersions => Expr::ProcessVersions,
        Expr::ProcessHrtimeBigint => Expr::ProcessHrtimeBigint,
        Expr::ProcessHrtime(a0) => Expr::ProcessHrtime(Shallow::shallow(a0, n)),
        Expr::ProcessNextTick { callback, args } => Expr::ProcessNextTick {
            callback: Shallow::shallow(callback, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProcessOn { event, handler } => Expr::ProcessOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessOnce { event, handler } => Expr::ProcessOnce {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessChdir(a0) => Expr::ProcessChdir(Shallow::shallow(a0, n)),
        Expr::ProcessKill { pid, signal } => Expr::ProcessKill {
            pid: Shallow::shallow(pid, n),
            signal: Shallow::shallow(signal, n),
        },
        Expr::ProcessExit(a0) => Expr::ProcessExit(Shallow::shallow(a0, n)),
        Expr::ProcessAbort => Expr::ProcessAbort,
        Expr::ProcessUmask(a0) => Expr::ProcessUmask(Shallow::shallow(a0, n)),
        Expr::ProcessThreadCpuUsage(a0) => Expr::ProcessThreadCpuUsage(Shallow::shallow(a0, n)),
        Expr::ProcessAvailableMemory => Expr::ProcessAvailableMemory,
        Expr::ProcessConstrainedMemory => Expr::ProcessConstrainedMemory,
        Expr::ProcessPosixCredential(a0) => Expr::ProcessPosixCredential(a0.clone()),
        Expr::ProcessEmitWarning(a0) => Expr::ProcessEmitWarning(Shallow::shallow(a0, n)),
        Expr::ProcessCpuUsage(a0) => Expr::ProcessCpuUsage(Shallow::shallow(a0, n)),
        Expr::ProcessResourceUsage => Expr::ProcessResourceUsage,
        Expr::ProcessActiveResourcesInfo => Expr::ProcessActiveResourcesInfo,
        Expr::ProcessTitle => Expr::ProcessTitle,
        Expr::ProcessSetTitle(a0) => Expr::ProcessSetTitle(Shallow::shallow(a0, n)),
        Expr::ProcessStdin => Expr::ProcessStdin,
        Expr::ProcessStdout => Expr::ProcessStdout,
        Expr::ProcessStderr => Expr::ProcessStderr,
        Expr::ProcessStdinSetRawMode(a0) => Expr::ProcessStdinSetRawMode(Shallow::shallow(a0, n)),
        Expr::ProcessStdinOn { event, handler } => Expr::ProcessStdinOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinRemoveListener { event, handler } => Expr::ProcessStdinRemoveListener {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinLifecycle(a0) => Expr::ProcessStdinLifecycle(a0.clone()),
        Expr::ProcessStdoutOn { event, handler } => Expr::ProcessStdoutOn {
            event: Shallow::shallow(event, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProcessStdinIsTTY => Expr::ProcessStdinIsTTY,
        Expr::ProcessStdoutIsTTY => Expr::ProcessStdoutIsTTY,
        Expr::ProcessStderrIsTTY => Expr::ProcessStderrIsTTY,
        Expr::ProcessStdoutColumns => Expr::ProcessStdoutColumns,
        Expr::ProcessStdoutRows => Expr::ProcessStdoutRows,
        Expr::TtyIsAtty(a0) => Expr::TtyIsAtty(Shallow::shallow(a0, n)),
        Expr::FsReadFileSync(a0) => Expr::FsReadFileSync(Shallow::shallow(a0, n)),
        Expr::FsWriteFileSync(a0, a1) => {
            Expr::FsWriteFileSync(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::FsExistsSync(a0) => Expr::FsExistsSync(Shallow::shallow(a0, n)),
        Expr::FsMkdirSync(a0) => Expr::FsMkdirSync(Shallow::shallow(a0, n)),
        Expr::FsUnlinkSync(a0) => Expr::FsUnlinkSync(Shallow::shallow(a0, n)),
        Expr::FsAppendFileSync(a0, a1) => {
            Expr::FsAppendFileSync(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::FsReadFileBinary(a0) => Expr::FsReadFileBinary(Shallow::shallow(a0, n)),
        Expr::FsRmRecursive(a0) => Expr::FsRmRecursive(Shallow::shallow(a0, n)),
        Expr::PathJoin(a0, a1) => Expr::PathJoin(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::PathDirname(a0) => Expr::PathDirname(Shallow::shallow(a0, n)),
        Expr::PathBasename(a0) => Expr::PathBasename(Shallow::shallow(a0, n)),
        Expr::PathBasenameExt(a0, a1) => {
            Expr::PathBasenameExt(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathExtname(a0) => Expr::PathExtname(Shallow::shallow(a0, n)),
        Expr::PathResolve(a0) => Expr::PathResolve(Shallow::shallow(a0, n)),
        Expr::PathIsAbsolute(a0) => Expr::PathIsAbsolute(Shallow::shallow(a0, n)),
        Expr::PathRelative(a0, a1) => {
            Expr::PathRelative(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathNormalize(a0) => Expr::PathNormalize(Shallow::shallow(a0, n)),
        Expr::PathParse(a0) => Expr::PathParse(Shallow::shallow(a0, n)),
        Expr::PathFormat(a0) => Expr::PathFormat(Shallow::shallow(a0, n)),
        Expr::PathSep => Expr::PathSep,
        Expr::PathDelimiter => Expr::PathDelimiter,
        Expr::PathToNamespacedPath(a0) => Expr::PathToNamespacedPath(Shallow::shallow(a0, n)),
        Expr::PathMatchesGlob(a0, a1) => {
            Expr::PathMatchesGlob(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathResolveJoin(a0, a1) => {
            Expr::PathResolveJoin(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathWin32Join(a0, a1) => {
            Expr::PathWin32Join(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::PathWin32 { method, args } => Expr::PathWin32 {
            method: method.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::WeakRefNew(a0) => Expr::WeakRefNew(Shallow::shallow(a0, n)),
        Expr::WeakRefDeref(a0) => Expr::WeakRefDeref(Shallow::shallow(a0, n)),
        Expr::FinalizationRegistryNew(a0) => Expr::FinalizationRegistryNew(Shallow::shallow(a0, n)),
        Expr::FinalizationRegistryRegister {
            registry,
            target,
            held,
            token,
        } => Expr::FinalizationRegistryRegister {
            registry: Shallow::shallow(registry, n),
            target: Shallow::shallow(target, n),
            held: Shallow::shallow(held, n),
            token: Shallow::shallow(token, n),
        },
        Expr::FinalizationRegistryUnregister { registry, token } => {
            Expr::FinalizationRegistryUnregister {
                registry: Shallow::shallow(registry, n),
                token: Shallow::shallow(token, n),
            }
        }
        Expr::ObjectDefineProperty(a0, a1, a2) => Expr::ObjectDefineProperty(
            Shallow::shallow(a0, n),
            Shallow::shallow(a1, n),
            Shallow::shallow(a2, n),
        ),
        Expr::ObjectGetOwnPropertyDescriptor(a0, a1) => {
            Expr::ObjectGetOwnPropertyDescriptor(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectGetOwnPropertyDescriptors(a0) => {
            Expr::ObjectGetOwnPropertyDescriptors(Shallow::shallow(a0, n))
        }
        Expr::ObjectGetOwnPropertyNames(a0) => {
            Expr::ObjectGetOwnPropertyNames(Shallow::shallow(a0, n))
        }
        Expr::ObjectCreate(a0, a1) => {
            Expr::ObjectCreate(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectFreeze(a0) => Expr::ObjectFreeze(Shallow::shallow(a0, n)),
        Expr::ObjectSeal(a0) => Expr::ObjectSeal(Shallow::shallow(a0, n)),
        Expr::ObjectPreventExtensions(a0) => Expr::ObjectPreventExtensions(Shallow::shallow(a0, n)),
        Expr::ObjectIsFrozen(a0) => Expr::ObjectIsFrozen(Shallow::shallow(a0, n)),
        Expr::ObjectIsSealed(a0) => Expr::ObjectIsSealed(Shallow::shallow(a0, n)),
        Expr::ObjectIsExtensible(a0) => Expr::ObjectIsExtensible(Shallow::shallow(a0, n)),
        Expr::ObjectGetPrototypeOf(a0) => Expr::ObjectGetPrototypeOf(Shallow::shallow(a0, n)),
        Expr::ObjectSetPrototypeOf(a0, a1) => {
            Expr::ObjectSetPrototypeOf(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectDefineProperties(a0, a1) => {
            Expr::ObjectDefineProperties(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectGetOwnPropertySymbols(a0) => {
            Expr::ObjectGetOwnPropertySymbols(Shallow::shallow(a0, n))
        }
        Expr::SymbolNew(a0) => Expr::SymbolNew(Shallow::shallow(a0, n)),
        Expr::SymbolFor(a0) => Expr::SymbolFor(Shallow::shallow(a0, n)),
        Expr::SymbolKeyFor(a0) => Expr::SymbolKeyFor(Shallow::shallow(a0, n)),
        Expr::SymbolDescription(a0) => Expr::SymbolDescription(Shallow::shallow(a0, n)),
        Expr::RegExpEscape(a0) => Expr::RegExpEscape(Shallow::shallow(a0, n)),
        Expr::SymbolToString(a0) => Expr::SymbolToString(Shallow::shallow(a0, n)),
        Expr::FileURLToPath(a0) => Expr::FileURLToPath(Shallow::shallow(a0, n)),
        Expr::RegExpExecIndex => Expr::RegExpExecIndex,
        Expr::RegExpExecGroups => Expr::RegExpExecGroups,
        Expr::JsonParse(a0) => Expr::JsonParse(Shallow::shallow(a0, n)),
        Expr::JsonParseTyped {
            text,
            ty,
            ordered_keys,
        } => Expr::JsonParseTyped {
            text: Shallow::shallow(text, n),
            ty: ty.clone(),
            ordered_keys: ordered_keys.clone(),
        },
        Expr::JsonParseReviver { text, reviver } => Expr::JsonParseReviver {
            text: Shallow::shallow(text, n),
            reviver: Shallow::shallow(reviver, n),
        },
        Expr::JsonParseWithReviver(a0, a1) => {
            Expr::JsonParseWithReviver(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::JsonStringify(a0) => Expr::JsonStringify(Shallow::shallow(a0, n)),
        Expr::JsonStringifyPretty {
            value,
            replacer,
            space,
        } => Expr::JsonStringifyPretty {
            value: Shallow::shallow(value, n),
            replacer: Shallow::shallow(replacer, n),
            space: Shallow::shallow(space, n),
        },
        Expr::JsonStringifyFull(a0, a1, a2) => Expr::JsonStringifyFull(
            Shallow::shallow(a0, n),
            Shallow::shallow(a1, n),
            Shallow::shallow(a2, n),
        ),
        Expr::JsonRawJson(a0) => Expr::JsonRawJson(Shallow::shallow(a0, n)),
        Expr::JsonIsRawJson(a0) => Expr::JsonIsRawJson(Shallow::shallow(a0, n)),
        Expr::MathFloor(a0) => Expr::MathFloor(Shallow::shallow(a0, n)),
        Expr::MathCeil(a0) => Expr::MathCeil(Shallow::shallow(a0, n)),
        Expr::MathRound(a0) => Expr::MathRound(Shallow::shallow(a0, n)),
        Expr::MathTrunc(a0) => Expr::MathTrunc(Shallow::shallow(a0, n)),
        Expr::MathSign(a0) => Expr::MathSign(Shallow::shallow(a0, n)),
        Expr::MathAbs(a0) => Expr::MathAbs(Shallow::shallow(a0, n)),
        Expr::MathSqrt(a0) => Expr::MathSqrt(Shallow::shallow(a0, n)),
        Expr::MathLog(a0) => Expr::MathLog(Shallow::shallow(a0, n)),
        Expr::MathLog2(a0) => Expr::MathLog2(Shallow::shallow(a0, n)),
        Expr::MathLog10(a0) => Expr::MathLog10(Shallow::shallow(a0, n)),
        Expr::MathPow(a0, a1) => Expr::MathPow(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::MathMin(a0) => Expr::MathMin(Shallow::shallow(a0, n)),
        Expr::MathMax(a0) => Expr::MathMax(Shallow::shallow(a0, n)),
        Expr::MathMinSpread(a0) => Expr::MathMinSpread(Shallow::shallow(a0, n)),
        Expr::MathMaxSpread(a0) => Expr::MathMaxSpread(Shallow::shallow(a0, n)),
        Expr::MathImul(a0, a1) => Expr::MathImul(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::MathRandom => Expr::MathRandom,
        Expr::MathSin(a0) => Expr::MathSin(Shallow::shallow(a0, n)),
        Expr::MathCos(a0) => Expr::MathCos(Shallow::shallow(a0, n)),
        Expr::MathTan(a0) => Expr::MathTan(Shallow::shallow(a0, n)),
        Expr::MathAsin(a0) => Expr::MathAsin(Shallow::shallow(a0, n)),
        Expr::MathAcos(a0) => Expr::MathAcos(Shallow::shallow(a0, n)),
        Expr::MathAtan(a0) => Expr::MathAtan(Shallow::shallow(a0, n)),
        Expr::MathAtan2(a0, a1) => {
            Expr::MathAtan2(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::MathCbrt(a0) => Expr::MathCbrt(Shallow::shallow(a0, n)),
        Expr::MathHypot(a0) => Expr::MathHypot(Shallow::shallow(a0, n)),
        Expr::MathFround(a0) => Expr::MathFround(Shallow::shallow(a0, n)),
        Expr::MathF16round(a0) => Expr::MathF16round(Shallow::shallow(a0, n)),
        Expr::MathClz32(a0) => Expr::MathClz32(Shallow::shallow(a0, n)),
        Expr::MathExpm1(a0) => Expr::MathExpm1(Shallow::shallow(a0, n)),
        Expr::MathLog1p(a0) => Expr::MathLog1p(Shallow::shallow(a0, n)),
        Expr::MathSinh(a0) => Expr::MathSinh(Shallow::shallow(a0, n)),
        Expr::MathCosh(a0) => Expr::MathCosh(Shallow::shallow(a0, n)),
        Expr::MathTanh(a0) => Expr::MathTanh(Shallow::shallow(a0, n)),
        Expr::MathAsinh(a0) => Expr::MathAsinh(Shallow::shallow(a0, n)),
        Expr::MathAcosh(a0) => Expr::MathAcosh(Shallow::shallow(a0, n)),
        Expr::MathAtanh(a0) => Expr::MathAtanh(Shallow::shallow(a0, n)),
        Expr::MathExp(a0) => Expr::MathExp(Shallow::shallow(a0, n)),
        Expr::PerformanceNow => Expr::PerformanceNow,
        Expr::WebAssemblyValidate(a0) => Expr::WebAssemblyValidate(Shallow::shallow(a0, n)),
        Expr::WebAssemblyCompile(a0) => Expr::WebAssemblyCompile(Shallow::shallow(a0, n)),
        Expr::WebAssemblyModuleNew(a0) => Expr::WebAssemblyModuleNew(Shallow::shallow(a0, n)),
        Expr::WebAssemblyModuleExports(a0) => {
            Expr::WebAssemblyModuleExports(Shallow::shallow(a0, n))
        }
        Expr::WebAssemblyModuleImports(a0) => {
            Expr::WebAssemblyModuleImports(Shallow::shallow(a0, n))
        }
        Expr::WebAssemblyModuleCustomSections { module, name } => {
            Expr::WebAssemblyModuleCustomSections {
                module: Shallow::shallow(module, n),
                name: Shallow::shallow(name, n),
            }
        }
        Expr::WebAssemblyInstantiate { bytes, imports } => Expr::WebAssemblyInstantiate {
            bytes: Shallow::shallow(bytes, n),
            imports: Shallow::shallow(imports, n),
        },
        Expr::WebAssemblyCallExport {
            instance,
            name,
            args,
        } => Expr::WebAssemblyCallExport {
            instance: Shallow::shallow(instance, n),
            name: Shallow::shallow(name, n),
            args: Shallow::shallow(args, n),
        },
        Expr::Atob(a0) => Expr::Atob(Shallow::shallow(a0, n)),
        Expr::Btoa(a0) => Expr::Btoa(Shallow::shallow(a0, n)),
        Expr::TextEncoderNew => Expr::TextEncoderNew,
        Expr::TextEncoderEncode(a0) => Expr::TextEncoderEncode(Shallow::shallow(a0, n)),
        Expr::TextEncoderEncodeInto { source, dest } => Expr::TextEncoderEncodeInto {
            source: Shallow::shallow(source, n),
            dest: Shallow::shallow(dest, n),
        },
        Expr::TextDecoderNew {
            label,
            fatal,
            ignore_bom,
        } => Expr::TextDecoderNew {
            label: Shallow::shallow(label, n),
            fatal: Shallow::shallow(fatal, n),
            ignore_bom: Shallow::shallow(ignore_bom, n),
        },
        Expr::TextDecoderDecode { decoder, input } => Expr::TextDecoderDecode {
            decoder: Shallow::shallow(decoder, n),
            input: Shallow::shallow(input, n),
        },
        Expr::TextDecoderEncoding(a0) => Expr::TextDecoderEncoding(Shallow::shallow(a0, n)),
        Expr::TextDecoderFatal(a0) => Expr::TextDecoderFatal(Shallow::shallow(a0, n)),
        Expr::TextDecoderIgnoreBom(a0) => Expr::TextDecoderIgnoreBom(Shallow::shallow(a0, n)),
        Expr::EncodeURI(a0) => Expr::EncodeURI(Shallow::shallow(a0, n)),
        Expr::DecodeURI(a0) => Expr::DecodeURI(Shallow::shallow(a0, n)),
        Expr::EncodeURIComponent(a0) => Expr::EncodeURIComponent(Shallow::shallow(a0, n)),
        Expr::DecodeURIComponent(a0) => Expr::DecodeURIComponent(Shallow::shallow(a0, n)),
        Expr::StructuredClone { value, options } => Expr::StructuredClone {
            value: Shallow::shallow(value, n),
            options: Shallow::shallow(options, n),
        },
        Expr::LinkGeneratorPrototype {
            obj,
            is_async,
            fixed_state,
            i32_controls,
            i1_controls,
        } => Expr::LinkGeneratorPrototype {
            obj: Shallow::shallow(obj, n),
            is_async: is_async.clone(),
            fixed_state: fixed_state.clone(),
            i32_controls: i32_controls.clone(),
            i1_controls: i1_controls.clone(),
        },
        Expr::QueueMicrotask(a0) => Expr::QueueMicrotask(Shallow::shallow(a0, n)),
        Expr::IterResultSet(a0, a1) => Expr::IterResultSet(Shallow::shallow(a0, n), a1.clone()),
        Expr::IterResultGetValue => Expr::IterResultGetValue,
        Expr::IterResultGetDone => Expr::IterResultGetDone,
        Expr::AsyncStepChain {
            value,
            step_closure,
        } => Expr::AsyncStepChain {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncStepResumeNow {
            value,
            step_closure,
        } => Expr::AsyncStepResumeNow {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncStepDeclined(a0) => Expr::AsyncStepDeclined(Shallow::shallow(a0, n)),
        Expr::AsyncStepDone {
            value,
            step_closure,
        } => Expr::AsyncStepDone {
            value: Shallow::shallow(value, n),
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::CurrentStepClosure => Expr::CurrentStepClosure,
        Expr::AsyncFirstCall { step_closure } => Expr::AsyncFirstCall {
            step_closure: Shallow::shallow(step_closure, n),
        },
        Expr::AsyncGenResume {
            step_closure,
            value,
            is_error,
        } => Expr::AsyncGenResume {
            step_closure: Shallow::shallow(step_closure, n),
            value: Shallow::shallow(value, n),
            is_error: is_error.clone(),
        },
        Expr::CryptoRandomBytes(a0) => Expr::CryptoRandomBytes(Shallow::shallow(a0, n)),
        Expr::CryptoRandomUUID => Expr::CryptoRandomUUID,
        Expr::CryptoRandomUUIDv7 => Expr::CryptoRandomUUIDv7,
        Expr::CryptoSha256(a0) => Expr::CryptoSha256(Shallow::shallow(a0, n)),
        Expr::CryptoMd5(a0) => Expr::CryptoMd5(Shallow::shallow(a0, n)),
        Expr::WebCryptoDigest { algo, data } => Expr::WebCryptoDigest {
            algo: Shallow::shallow(algo, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoImportKey {
            format,
            key,
            algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoImportKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
            algorithm: Shallow::shallow(algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoExportKey { format, key } => Expr::WebCryptoExportKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
        },
        Expr::WebCryptoSign {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoSign {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoVerify {
            algorithm,
            key,
            signature,
            data,
        } => Expr::WebCryptoVerify {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            signature: Shallow::shallow(signature, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoDeriveBits {
            algorithm,
            base_key,
            length,
        } => Expr::WebCryptoDeriveBits {
            algorithm: Shallow::shallow(algorithm, n),
            base_key: Shallow::shallow(base_key, n),
            length: Shallow::shallow(length, n),
        },
        Expr::WebCryptoDeriveKey {
            algorithm,
            base_key,
            derived_key_algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoDeriveKey {
            algorithm: Shallow::shallow(algorithm, n),
            base_key: Shallow::shallow(base_key, n),
            derived_key_algorithm: Shallow::shallow(derived_key_algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoEncrypt {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoEncrypt {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoDecrypt {
            algorithm,
            key,
            data,
        } => Expr::WebCryptoDecrypt {
            algorithm: Shallow::shallow(algorithm, n),
            key: Shallow::shallow(key, n),
            data: Shallow::shallow(data, n),
        },
        Expr::WebCryptoGenerateKey {
            algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoGenerateKey {
            algorithm: Shallow::shallow(algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::WebCryptoWrapKey {
            format,
            key,
            wrapping_key,
            wrap_algorithm,
        } => Expr::WebCryptoWrapKey {
            format: Shallow::shallow(format, n),
            key: Shallow::shallow(key, n),
            wrapping_key: Shallow::shallow(wrapping_key, n),
            wrap_algorithm: Shallow::shallow(wrap_algorithm, n),
        },
        Expr::WebCryptoUnwrapKey {
            format,
            wrapped_key,
            unwrapping_key,
            unwrap_algorithm,
            unwrapped_key_algorithm,
            extractable,
            usages,
        } => Expr::WebCryptoUnwrapKey {
            format: Shallow::shallow(format, n),
            wrapped_key: Shallow::shallow(wrapped_key, n),
            unwrapping_key: Shallow::shallow(unwrapping_key, n),
            unwrap_algorithm: Shallow::shallow(unwrap_algorithm, n),
            unwrapped_key_algorithm: Shallow::shallow(unwrapped_key_algorithm, n),
            extractable: Shallow::shallow(extractable, n),
            usages: Shallow::shallow(usages, n),
        },
        Expr::CryptoRandomFillSync {
            buffer,
            offset,
            size,
        } => Expr::CryptoRandomFillSync {
            buffer: Shallow::shallow(buffer, n),
            offset: Shallow::shallow(offset, n),
            size: Shallow::shallow(size, n),
        },
        Expr::OsPlatform => Expr::OsPlatform,
        Expr::OsArch => Expr::OsArch,
        Expr::OsHostname => Expr::OsHostname,
        Expr::OsHomedir => Expr::OsHomedir,
        Expr::OsTmpdir => Expr::OsTmpdir,
        Expr::OsTotalmem => Expr::OsTotalmem,
        Expr::OsFreemem => Expr::OsFreemem,
        Expr::OsUptime => Expr::OsUptime,
        Expr::OsType => Expr::OsType,
        Expr::OsRelease => Expr::OsRelease,
        Expr::OsCpus => Expr::OsCpus,
        Expr::OsNetworkInterfaces => Expr::OsNetworkInterfaces,
        Expr::OsUserInfo => Expr::OsUserInfo,
        Expr::OsUserInfoBuffer => Expr::OsUserInfoBuffer,
        Expr::OsEOL => Expr::OsEOL,
        Expr::OsDevNull => Expr::OsDevNull,
        Expr::OsAvailableParallelism => Expr::OsAvailableParallelism,
        Expr::OsEndianness => Expr::OsEndianness,
        Expr::OsLoadavg => Expr::OsLoadavg,
        Expr::OsMachine => Expr::OsMachine,
        Expr::OsVersion => Expr::OsVersion,
        Expr::BufferFrom { data, encoding } => Expr::BufferFrom {
            data: Shallow::shallow(data, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferFromArrayBuffer {
            data,
            byte_offset,
            length,
        } => Expr::BufferFromArrayBuffer {
            data: Shallow::shallow(data, n),
            byte_offset: Shallow::shallow(byte_offset, n),
            length: Shallow::shallow(length, n),
        },
        Expr::BufferAlloc {
            size,
            fill,
            encoding,
        } => Expr::BufferAlloc {
            size: Shallow::shallow(size, n),
            fill: Shallow::shallow(fill, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferAllocUnsafe(a0) => Expr::BufferAllocUnsafe(Shallow::shallow(a0, n)),
        Expr::BufferConcat(a0) => Expr::BufferConcat(Shallow::shallow(a0, n)),
        Expr::BufferConcatWithLength { list, total_length } => Expr::BufferConcatWithLength {
            list: Shallow::shallow(list, n),
            total_length: Shallow::shallow(total_length, n),
        },
        Expr::BufferIsBuffer(a0) => Expr::BufferIsBuffer(Shallow::shallow(a0, n)),
        Expr::BufferIsEncoding(a0) => Expr::BufferIsEncoding(Shallow::shallow(a0, n)),
        Expr::BufferByteLength { data, encoding } => Expr::BufferByteLength {
            data: Shallow::shallow(data, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferToString { buffer, encoding } => Expr::BufferToString {
            buffer: Shallow::shallow(buffer, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferLength(a0) => Expr::BufferLength(Shallow::shallow(a0, n)),
        Expr::BufferSlice { buffer, start, end } => Expr::BufferSlice {
            buffer: Shallow::shallow(buffer, n),
            start: Shallow::shallow(start, n),
            end: Shallow::shallow(end, n),
        },
        Expr::BufferCopy {
            source,
            target,
            target_start,
            source_start,
            source_end,
        } => Expr::BufferCopy {
            source: Shallow::shallow(source, n),
            target: Shallow::shallow(target, n),
            target_start: Shallow::shallow(target_start, n),
            source_start: Shallow::shallow(source_start, n),
            source_end: Shallow::shallow(source_end, n),
        },
        Expr::BufferWrite {
            buffer,
            string,
            offset,
            encoding,
        } => Expr::BufferWrite {
            buffer: Shallow::shallow(buffer, n),
            string: Shallow::shallow(string, n),
            offset: Shallow::shallow(offset, n),
            encoding: Shallow::shallow(encoding, n),
        },
        Expr::BufferFill { buffer, value } => Expr::BufferFill {
            buffer: Shallow::shallow(buffer, n),
            value: Shallow::shallow(value, n),
        },
        Expr::BufferEquals { buffer, other } => Expr::BufferEquals {
            buffer: Shallow::shallow(buffer, n),
            other: Shallow::shallow(other, n),
        },
        Expr::BufferIndexGet { buffer, index } => Expr::BufferIndexGet {
            buffer: Shallow::shallow(buffer, n),
            index: Shallow::shallow(index, n),
        },
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => Expr::BufferIndexSet {
            buffer: Shallow::shallow(buffer, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
        },
        Expr::Uint8ArrayNew(a0) => Expr::Uint8ArrayNew(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayFrom(a0) => Expr::Uint8ArrayFrom(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayLength(a0) => Expr::Uint8ArrayLength(Shallow::shallow(a0, n)),
        Expr::Uint8ArrayGet { array, index } => Expr::Uint8ArrayGet {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
        },
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => Expr::Uint8ArraySet {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
        },
        Expr::TypedArrayNew { kind, arg } => Expr::TypedArrayNew {
            kind: kind.clone(),
            arg: Shallow::shallow(arg, n),
        },
        Expr::NativeArenaAlloc(a0) => Expr::NativeArenaAlloc(Shallow::shallow(a0, n)),
        Expr::NativeArenaView {
            owner,
            kind,
            byte_offset,
            length,
        } => Expr::NativeArenaView {
            owner: Shallow::shallow(owner, n),
            kind: kind.clone(),
            byte_offset: Shallow::shallow(byte_offset, n),
            length: Shallow::shallow(length, n),
        },
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            view_type,
        } => Expr::NativePodView {
            owner: Shallow::shallow(owner, n),
            byte_offset: Shallow::shallow(byte_offset, n),
            count: Shallow::shallow(count, n),
            view_type: view_type.clone(),
        },
        Expr::PodLayoutSizeOf { ty } => Expr::PodLayoutSizeOf { ty: ty.clone() },
        Expr::PodLayoutAlignOf { ty } => Expr::PodLayoutAlignOf { ty: ty.clone() },
        Expr::PodLayoutOffsetOf { ty, field_path } => Expr::PodLayoutOffsetOf {
            ty: ty.clone(),
            field_path: field_path.clone(),
        },
        Expr::NativeArenaDispose(a0) => Expr::NativeArenaDispose(Shallow::shallow(a0, n)),
        Expr::NativeMemoryFillU32 { view, value } => Expr::NativeMemoryFillU32 {
            view: Shallow::shallow(view, n),
            value: Shallow::shallow(value, n),
        },
        Expr::NativeMemoryCopy { dst, src } => Expr::NativeMemoryCopy {
            dst: Shallow::shallow(dst, n),
            src: Shallow::shallow(src, n),
        },
        Expr::ChildProcessExecSync { command, options } => Expr::ChildProcessExecSync {
            command: Shallow::shallow(command, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawnSync {
            command,
            args,
            options,
        } => Expr::ChildProcessSpawnSync {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawn {
            command,
            args,
            options,
        } => Expr::ChildProcessSpawn {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessFork {
            module,
            args,
            options,
        } => Expr::ChildProcessFork {
            module: Shallow::shallow(module, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessExec {
            command,
            options,
            callback,
        } => Expr::ChildProcessExec {
            command: Shallow::shallow(command, n),
            options: Shallow::shallow(options, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ChildProcessExecFile {
            file,
            args,
            options,
            callback,
        } => Expr::ChildProcessExecFile {
            file: Shallow::shallow(file, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ChildProcessExecFileSync {
            file,
            args,
            options,
        } => Expr::ChildProcessExecFileSync {
            file: Shallow::shallow(file, n),
            args: Shallow::shallow(args, n),
            options: Shallow::shallow(options, n),
        },
        Expr::ChildProcessSpawnBackground {
            command,
            args,
            log_file,
            env_json,
        } => Expr::ChildProcessSpawnBackground {
            command: Shallow::shallow(command, n),
            args: Shallow::shallow(args, n),
            log_file: Shallow::shallow(log_file, n),
            env_json: Shallow::shallow(env_json, n),
        },
        Expr::ChildProcessGetProcessStatus(a0) => {
            Expr::ChildProcessGetProcessStatus(Shallow::shallow(a0, n))
        }
        Expr::ChildProcessKillProcess(a0) => Expr::ChildProcessKillProcess(Shallow::shallow(a0, n)),
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
            redirect,
        } => Expr::FetchWithOptions {
            url: Shallow::shallow(url, n),
            method: Shallow::shallow(method, n),
            body: Shallow::shallow(body, n),
            headers: Shallow::shallow(headers, n),
            headers_dynamic: Shallow::shallow(headers_dynamic, n),
            signal: Shallow::shallow(signal, n),
            redirect: Shallow::shallow(redirect, n),
        },
        Expr::FetchGetWithAuth { url, auth_header } => Expr::FetchGetWithAuth {
            url: Shallow::shallow(url, n),
            auth_header: Shallow::shallow(auth_header, n),
        },
        Expr::FetchPostWithAuth {
            url,
            auth_header,
            body,
        } => Expr::FetchPostWithAuth {
            url: Shallow::shallow(url, n),
            auth_header: Shallow::shallow(auth_header, n),
            body: Shallow::shallow(body, n),
        },
        Expr::NetCreateServer {
            options,
            connection_listener,
        } => Expr::NetCreateServer {
            options: Shallow::shallow(options, n),
            connection_listener: Shallow::shallow(connection_listener, n),
        },
        Expr::NetCreateConnection {
            port,
            host,
            connect_listener,
        } => Expr::NetCreateConnection {
            port: Shallow::shallow(port, n),
            host: Shallow::shallow(host, n),
            connect_listener: Shallow::shallow(connect_listener, n),
        },
        Expr::NetConnect {
            port,
            host,
            connect_listener,
        } => Expr::NetConnect {
            port: Shallow::shallow(port, n),
            host: Shallow::shallow(host, n),
            connect_listener: Shallow::shallow(connect_listener, n),
        },
        Expr::ArrayPush {
            array_id,
            value,
            field_writeback,
        } => Expr::ArrayPush {
            array_id: array_id.clone(),
            value: Shallow::shallow(value, n),
            field_writeback: field_writeback.clone(),
        },
        Expr::ArrayPushSpread { array_id, source } => Expr::ArrayPushSpread {
            array_id: array_id.clone(),
            source: Shallow::shallow(source, n),
        },
        Expr::ArrayPop(a0) => Expr::ArrayPop(a0.clone()),
        Expr::ArrayShift(a0) => Expr::ArrayShift(a0.clone()),
        Expr::ArrayUnshift { array_id, value } => Expr::ArrayUnshift {
            array_id: array_id.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => Expr::ArrayIndexOf {
            array: Shallow::shallow(array, n),
            value: Shallow::shallow(value, n),
            from_index: Shallow::shallow(from_index, n),
        },
        Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => Expr::ArrayLastIndexOf {
            array: Shallow::shallow(array, n),
            value: Shallow::shallow(value, n),
            from_index: Shallow::shallow(from_index, n),
        },
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => Expr::ArrayIncludes {
            array: Shallow::shallow(array, n),
            value: Shallow::shallow(value, n),
            from_index: Shallow::shallow(from_index, n),
        },
        Expr::ArraySlice {
            array,
            start,
            end,
            layout,
        } => Expr::ArraySlice {
            array: Shallow::shallow(array, n),
            start: Shallow::shallow(start, n),
            end: Shallow::shallow(end, n),
            layout: layout.clone(),
        },
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => Expr::ArraySplice {
            array_id: array_id.clone(),
            start: Shallow::shallow(start, n),
            delete_count: Shallow::shallow(delete_count, n),
            items: Shallow::shallow(items, n),
        },
        Expr::ArrayForEach { array, callback } => Expr::ArrayForEach {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayMap { array, callback } => Expr::ArrayMap {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFilter { array, callback } => Expr::ArrayFilter {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFind { array, callback } => Expr::ArrayFind {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFindIndex { array, callback } => Expr::ArrayFindIndex {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFindLast { array, callback } => Expr::ArrayFindLast {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFindLastIndex { array, callback } => Expr::ArrayFindLastIndex {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayAt { array, index } => Expr::ArrayAt {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
        },
        Expr::ArraySome { array, callback } => Expr::ArraySome {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayEvery { array, callback } => Expr::ArrayEvery {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArrayFlatMap { array, callback } => Expr::ArrayFlatMap {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
        },
        Expr::ArraySort { array, comparator } => Expr::ArraySort {
            array: Shallow::shallow(array, n),
            comparator: Shallow::shallow(comparator, n),
        },
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        } => Expr::ArrayReduce {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
            initial: Shallow::shallow(initial, n),
        },
        Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => Expr::ArrayReduceRight {
            array: Shallow::shallow(array, n),
            callback: Shallow::shallow(callback, n),
            initial: Shallow::shallow(initial, n),
        },
        Expr::ArrayJoin { array, separator } => Expr::ArrayJoin {
            array: Shallow::shallow(array, n),
            separator: Shallow::shallow(separator, n),
        },
        Expr::ArrayFlat { array } => Expr::ArrayFlat {
            array: Shallow::shallow(array, n),
        },
        Expr::ArrayToReversed { array } => Expr::ArrayToReversed {
            array: Shallow::shallow(array, n),
        },
        Expr::ArrayToSorted { array, comparator } => Expr::ArrayToSorted {
            array: Shallow::shallow(array, n),
            comparator: Shallow::shallow(comparator, n),
        },
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => Expr::ArrayToSpliced {
            array: Shallow::shallow(array, n),
            start: Shallow::shallow(start, n),
            delete_count: Shallow::shallow(delete_count, n),
            items: Shallow::shallow(items, n),
        },
        Expr::ArrayWith {
            array,
            index,
            value,
        } => Expr::ArrayWith {
            array: Shallow::shallow(array, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
        },
        Expr::ArrayReverseValue { receiver } => Expr::ArrayReverseValue {
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => Expr::ArrayCopyWithin {
            array_id: array_id.clone(),
            target: Shallow::shallow(target, n),
            start: Shallow::shallow(start, n),
            end: Shallow::shallow(end, n),
        },
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => Expr::ArrayCopyWithinValue {
            receiver: Shallow::shallow(receiver, n),
            target: Shallow::shallow(target, n),
            start: Shallow::shallow(start, n),
            end: Shallow::shallow(end, n),
        },
        Expr::ArrayEntries(a0) => Expr::ArrayEntries(Shallow::shallow(a0, n)),
        Expr::ArrayKeys(a0) => Expr::ArrayKeys(Shallow::shallow(a0, n)),
        Expr::ArrayValues(a0) => Expr::ArrayValues(Shallow::shallow(a0, n)),
        Expr::ArrayLikeMethod {
            method,
            receiver,
            args,
            source,
        } => Expr::ArrayLikeMethod {
            method: method.clone(),
            receiver: Shallow::shallow(receiver, n),
            args: Shallow::shallow(args, n),
            source: source.clone(),
        },
        Expr::StringSplit(a0, a1) => {
            Expr::StringSplit(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::StringFromCharCode(a0) => Expr::StringFromCharCode(Shallow::shallow(a0, n)),
        Expr::StringFromCharCodeSpread(a0) => {
            Expr::StringFromCharCodeSpread(Shallow::shallow(a0, n))
        }
        Expr::StringFromCodePoint(a0) => Expr::StringFromCodePoint(Shallow::shallow(a0, n)),
        Expr::StringRaw {
            call_site,
            substitutions,
        } => Expr::StringRaw {
            call_site: Shallow::shallow(call_site, n),
            substitutions: Shallow::shallow(substitutions, n),
        },
        Expr::StringAt { string, index } => Expr::StringAt {
            string: Shallow::shallow(string, n),
            index: Shallow::shallow(index, n),
        },
        Expr::StringCodePointAt { string, index } => Expr::StringCodePointAt {
            string: Shallow::shallow(string, n),
            index: Shallow::shallow(index, n),
        },
        Expr::MapNew => Expr::MapNew,
        Expr::MapNewFromArray(a0) => Expr::MapNewFromArray(Shallow::shallow(a0, n)),
        Expr::MapSet { map, key, value } => Expr::MapSet {
            map: Shallow::shallow(map, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
        },
        Expr::MapGet { map, key } => Expr::MapGet {
            map: Shallow::shallow(map, n),
            key: Shallow::shallow(key, n),
        },
        Expr::MapHas { map, key } => Expr::MapHas {
            map: Shallow::shallow(map, n),
            key: Shallow::shallow(key, n),
        },
        Expr::MapDelete { map, key } => Expr::MapDelete {
            map: Shallow::shallow(map, n),
            key: Shallow::shallow(key, n),
        },
        Expr::MapSize(a0) => Expr::MapSize(Shallow::shallow(a0, n)),
        Expr::MapClear(a0) => Expr::MapClear(Shallow::shallow(a0, n)),
        Expr::MapEntries(a0) => Expr::MapEntries(Shallow::shallow(a0, n)),
        Expr::MapKeys(a0) => Expr::MapKeys(Shallow::shallow(a0, n)),
        Expr::MapValues(a0) => Expr::MapValues(Shallow::shallow(a0, n)),
        Expr::MapEntryKeyAt { map, idx } => Expr::MapEntryKeyAt {
            map: Shallow::shallow(map, n),
            idx: Shallow::shallow(idx, n),
        },
        Expr::MapEntryValueAt { map, idx } => Expr::MapEntryValueAt {
            map: Shallow::shallow(map, n),
            idx: Shallow::shallow(idx, n),
        },
        Expr::SetNew => Expr::SetNew,
        Expr::SetNewFromArray(a0) => Expr::SetNewFromArray(Shallow::shallow(a0, n)),
        Expr::SetAdd { set_id, value } => Expr::SetAdd {
            set_id: set_id.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::SetHas { set, value } => Expr::SetHas {
            set: Shallow::shallow(set, n),
            value: Shallow::shallow(value, n),
        },
        Expr::SetDelete { set, value } => Expr::SetDelete {
            set: Shallow::shallow(set, n),
            value: Shallow::shallow(value, n),
        },
        Expr::SetSize(a0) => Expr::SetSize(Shallow::shallow(a0, n)),
        Expr::SetClear(a0) => Expr::SetClear(Shallow::shallow(a0, n)),
        Expr::SetValues(a0) => Expr::SetValues(Shallow::shallow(a0, n)),
        Expr::SetValueAt { set, idx } => Expr::SetValueAt {
            set: Shallow::shallow(set, n),
            idx: Shallow::shallow(idx, n),
        },
        Expr::Sequence(a0) => Expr::Sequence(Shallow::shallow(a0, n)),
        Expr::BoxedPrimitiveNew {
            kind,
            arg,
            arg_present,
        } => Expr::BoxedPrimitiveNew {
            kind: kind.clone(),
            arg: Shallow::shallow(arg, n),
            arg_present: arg_present.clone(),
        },
        Expr::DateNow => Expr::DateNow,
        Expr::DateNew(a0) => Expr::DateNew(Shallow::shallow(a0, n)),
        Expr::DateGetTime(a0) => Expr::DateGetTime(Shallow::shallow(a0, n)),
        Expr::DateToISOString(a0) => Expr::DateToISOString(Shallow::shallow(a0, n)),
        Expr::DateGetFullYear(a0) => Expr::DateGetFullYear(Shallow::shallow(a0, n)),
        Expr::DateGetMonth(a0) => Expr::DateGetMonth(Shallow::shallow(a0, n)),
        Expr::DateGetDate(a0) => Expr::DateGetDate(Shallow::shallow(a0, n)),
        Expr::DateGetDay(a0) => Expr::DateGetDay(Shallow::shallow(a0, n)),
        Expr::DateGetHours(a0) => Expr::DateGetHours(Shallow::shallow(a0, n)),
        Expr::DateGetMinutes(a0) => Expr::DateGetMinutes(Shallow::shallow(a0, n)),
        Expr::DateGetSeconds(a0) => Expr::DateGetSeconds(Shallow::shallow(a0, n)),
        Expr::DateGetMilliseconds(a0) => Expr::DateGetMilliseconds(Shallow::shallow(a0, n)),
        Expr::DateParse(a0) => Expr::DateParse(Shallow::shallow(a0, n)),
        Expr::DateUtc(a0) => Expr::DateUtc(Shallow::shallow(a0, n)),
        Expr::DateGetUtcDay(a0) => Expr::DateGetUtcDay(Shallow::shallow(a0, n)),
        Expr::DateGetUtcFullYear(a0) => Expr::DateGetUtcFullYear(Shallow::shallow(a0, n)),
        Expr::DateGetUtcMonth(a0) => Expr::DateGetUtcMonth(Shallow::shallow(a0, n)),
        Expr::DateGetUtcDate(a0) => Expr::DateGetUtcDate(Shallow::shallow(a0, n)),
        Expr::DateGetUtcHours(a0) => Expr::DateGetUtcHours(Shallow::shallow(a0, n)),
        Expr::DateGetUtcMinutes(a0) => Expr::DateGetUtcMinutes(Shallow::shallow(a0, n)),
        Expr::DateGetUtcSeconds(a0) => Expr::DateGetUtcSeconds(Shallow::shallow(a0, n)),
        Expr::DateGetUtcMilliseconds(a0) => Expr::DateGetUtcMilliseconds(Shallow::shallow(a0, n)),
        Expr::DateSetUtcFullYear { date, args } => Expr::DateSetUtcFullYear {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcMonth { date, args } => Expr::DateSetUtcMonth {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcDate { date, args } => Expr::DateSetUtcDate {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcHours { date, args } => Expr::DateSetUtcHours {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcMinutes { date, args } => Expr::DateSetUtcMinutes {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcSeconds { date, args } => Expr::DateSetUtcSeconds {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetUtcMilliseconds { date, args } => Expr::DateSetUtcMilliseconds {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetFullYear { date, args } => Expr::DateSetFullYear {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetMonth { date, args } => Expr::DateSetMonth {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetDate { date, args } => Expr::DateSetDate {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetHours { date, args } => Expr::DateSetHours {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetMinutes { date, args } => Expr::DateSetMinutes {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetSeconds { date, args } => Expr::DateSetSeconds {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetMilliseconds { date, args } => Expr::DateSetMilliseconds {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateSetTime { date, args } => Expr::DateSetTime {
            date: Shallow::shallow(date, n),
            args: Shallow::shallow(args, n),
        },
        Expr::DateValueOf(a0) => Expr::DateValueOf(Shallow::shallow(a0, n)),
        Expr::DateToString(a0) => Expr::DateToString(Shallow::shallow(a0, n)),
        Expr::DateToDateString(a0) => Expr::DateToDateString(Shallow::shallow(a0, n)),
        Expr::DateToTimeString(a0) => Expr::DateToTimeString(Shallow::shallow(a0, n)),
        Expr::DateToUTCString(a0) => Expr::DateToUTCString(Shallow::shallow(a0, n)),
        Expr::DateToLocaleDateString(a0) => Expr::DateToLocaleDateString(Shallow::shallow(a0, n)),
        Expr::DateToLocaleTimeString(a0) => Expr::DateToLocaleTimeString(Shallow::shallow(a0, n)),
        Expr::DateToLocaleString(a0) => Expr::DateToLocaleString(Shallow::shallow(a0, n)),
        Expr::DateGetTimezoneOffset(a0) => Expr::DateGetTimezoneOffset(Shallow::shallow(a0, n)),
        Expr::DateToJSON(a0) => Expr::DateToJSON(Shallow::shallow(a0, n)),
        Expr::ErrorNew(a0) => Expr::ErrorNew(Shallow::shallow(a0, n)),
        Expr::ErrorMessage(a0) => Expr::ErrorMessage(Shallow::shallow(a0, n)),
        Expr::ErrorNewWithCause { message, cause } => Expr::ErrorNewWithCause {
            message: Shallow::shallow(message, n),
            cause: Shallow::shallow(cause, n),
        },
        Expr::ErrorNewWithOptions {
            kind,
            message,
            options,
        } => Expr::ErrorNewWithOptions {
            kind: kind.clone(),
            message: Shallow::shallow(message, n),
            options: Shallow::shallow(options, n),
        },
        Expr::TypeErrorNew(a0) => Expr::TypeErrorNew(Shallow::shallow(a0, n)),
        Expr::RangeErrorNew(a0) => Expr::RangeErrorNew(Shallow::shallow(a0, n)),
        Expr::ReferenceErrorNew(a0) => Expr::ReferenceErrorNew(Shallow::shallow(a0, n)),
        Expr::SyntaxErrorNew(a0) => Expr::SyntaxErrorNew(Shallow::shallow(a0, n)),
        Expr::AggregateErrorNew {
            errors,
            message,
            options,
        } => Expr::AggregateErrorNew {
            errors: Shallow::shallow(errors, n),
            message: Shallow::shallow(message, n),
            options: Shallow::shallow(options, n),
        },
        Expr::UrlNew { url, base } => Expr::UrlNew {
            url: Shallow::shallow(url, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlPatternNew { input, base } => Expr::UrlPatternNew {
            input: Shallow::shallow(input, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlGetHref(a0) => Expr::UrlGetHref(Shallow::shallow(a0, n)),
        Expr::UrlGetPathname(a0) => Expr::UrlGetPathname(Shallow::shallow(a0, n)),
        Expr::UrlGetProtocol(a0) => Expr::UrlGetProtocol(Shallow::shallow(a0, n)),
        Expr::UrlGetHost(a0) => Expr::UrlGetHost(Shallow::shallow(a0, n)),
        Expr::UrlGetHostname(a0) => Expr::UrlGetHostname(Shallow::shallow(a0, n)),
        Expr::UrlGetPort(a0) => Expr::UrlGetPort(Shallow::shallow(a0, n)),
        Expr::UrlGetSearch(a0) => Expr::UrlGetSearch(Shallow::shallow(a0, n)),
        Expr::UrlGetHash(a0) => Expr::UrlGetHash(Shallow::shallow(a0, n)),
        Expr::UrlGetOrigin(a0) => Expr::UrlGetOrigin(Shallow::shallow(a0, n)),
        Expr::UrlGetSearchParams(a0) => Expr::UrlGetSearchParams(Shallow::shallow(a0, n)),
        Expr::UrlCanParse(a0) => Expr::UrlCanParse(Shallow::shallow(a0, n)),
        Expr::UrlCanParseWithBase { input, base } => Expr::UrlCanParseWithBase {
            input: Shallow::shallow(input, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlParse(a0) => Expr::UrlParse(Shallow::shallow(a0, n)),
        Expr::UrlParseWithBase { input, base } => Expr::UrlParseWithBase {
            input: Shallow::shallow(input, n),
            base: Shallow::shallow(base, n),
        },
        Expr::UrlInstanceToString(a0) => Expr::UrlInstanceToString(Shallow::shallow(a0, n)),
        Expr::UrlInstanceToJSON(a0) => Expr::UrlInstanceToJSON(Shallow::shallow(a0, n)),
        Expr::UrlSetPathname { url, value } => Expr::UrlSetPathname {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetSearch { url, value } => Expr::UrlSetSearch {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHash { url, value } => Expr::UrlSetHash {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetProtocol { url, value } => Expr::UrlSetProtocol {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHostname { url, value } => Expr::UrlSetHostname {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetPort { url, value } => Expr::UrlSetPort {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetUsername { url, value } => Expr::UrlSetUsername {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetPassword { url, value } => Expr::UrlSetPassword {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSetHref { url, value } => Expr::UrlSetHref {
            url: Shallow::shallow(url, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsNew(a0) => Expr::UrlSearchParamsNew(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsMissingArgs {
            params,
            args,
            name_and_value,
        } => Expr::UrlSearchParamsMissingArgs {
            params: Shallow::shallow(params, n),
            args: Shallow::shallow(args, n),
            name_and_value: name_and_value.clone(),
        },
        Expr::UrlSearchParamsGet { params, name } => Expr::UrlSearchParamsGet {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
        },
        Expr::UrlSearchParamsHas {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsHas {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsSet {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsSet {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsAppend {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsAppend {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsDelete {
            params,
            name,
            value,
        } => Expr::UrlSearchParamsDelete {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
            value: Shallow::shallow(value, n),
        },
        Expr::UrlSearchParamsToString(a0) => Expr::UrlSearchParamsToString(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsGetAll { params, name } => Expr::UrlSearchParamsGetAll {
            params: Shallow::shallow(params, n),
            name: Shallow::shallow(name, n),
        },
        Expr::UrlSearchParamsEntries(a0) => Expr::UrlSearchParamsEntries(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsKeys(a0) => Expr::UrlSearchParamsKeys(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsValues(a0) => Expr::UrlSearchParamsValues(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsSort(a0) => Expr::UrlSearchParamsSort(Shallow::shallow(a0, n)),
        Expr::UrlSearchParamsForEach {
            params,
            callback,
            this_arg,
        } => Expr::UrlSearchParamsForEach {
            params: Shallow::shallow(params, n),
            callback: Shallow::shallow(callback, n),
            this_arg: Shallow::shallow(this_arg, n),
        },
        Expr::Delete(a0) => Expr::Delete(Shallow::shallow(a0, n)),
        Expr::Closure {
            func_id,
            params,
            return_type,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target,
            enclosing_class,
            is_arrow,
            is_async,
            is_generator,
            is_strict,
        } => Expr::Closure {
            func_id: func_id.clone(),
            params: Shallow::shallow(params, n),
            return_type: return_type.clone(),
            body: body.clone(),
            captures: captures.clone(),
            mutable_captures: mutable_captures.clone(),
            captures_this: captures_this.clone(),
            captures_new_target: captures_new_target.clone(),
            enclosing_class: enclosing_class.clone(),
            is_arrow: is_arrow.clone(),
            is_async: is_async.clone(),
            is_generator: is_generator.clone(),
            is_strict: is_strict.clone(),
        },
        Expr::RegExp { pattern, flags } => Expr::RegExp {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::RegExpDynamic {
            pattern,
            flags,
            is_call,
        } => Expr::RegExpDynamic {
            pattern: Shallow::shallow(pattern, n),
            flags: Shallow::shallow(flags, n),
            is_call: is_call.clone(),
        },
        Expr::StringMatch { string, regex } => Expr::StringMatch {
            string: Shallow::shallow(string, n),
            regex: Shallow::shallow(regex, n),
        },
        Expr::StringMatchAll { string, regex } => Expr::StringMatchAll {
            string: Shallow::shallow(string, n),
            regex: Shallow::shallow(regex, n),
        },
        Expr::StringReplace {
            string,
            pattern,
            replacement,
        } => Expr::StringReplace {
            string: Shallow::shallow(string, n),
            pattern: Shallow::shallow(pattern, n),
            replacement: Shallow::shallow(replacement, n),
        },
        Expr::ObjectFromEntries(a0) => Expr::ObjectFromEntries(Shallow::shallow(a0, n)),
        Expr::ObjectIs(a0, a1) => Expr::ObjectIs(Shallow::shallow(a0, n), Shallow::shallow(a1, n)),
        Expr::ObjectHasOwn(a0, a1) => {
            Expr::ObjectHasOwn(Shallow::shallow(a0, n), Shallow::shallow(a1, n))
        }
        Expr::ObjectKeys(a0) => Expr::ObjectKeys(Shallow::shallow(a0, n)),
        Expr::ForInKeys(a0) => Expr::ForInKeys(Shallow::shallow(a0, n)),
        Expr::ObjectValues(a0) => Expr::ObjectValues(Shallow::shallow(a0, n)),
        Expr::ObjectEntries(a0) => Expr::ObjectEntries(Shallow::shallow(a0, n)),
        Expr::ObjectGroupBy { items, key_fn } => Expr::ObjectGroupBy {
            items: Shallow::shallow(items, n),
            key_fn: Shallow::shallow(key_fn, n),
        },
        Expr::MapGroupBy { items, key_fn } => Expr::MapGroupBy {
            items: Shallow::shallow(items, n),
            key_fn: Shallow::shallow(key_fn, n),
        },
        Expr::ObjectRest {
            object,
            exclude_keys,
        } => Expr::ObjectRest {
            object: Shallow::shallow(object, n),
            exclude_keys: exclude_keys.clone(),
        },
        Expr::ArrayIsArray(a0) => Expr::ArrayIsArray(Shallow::shallow(a0, n)),
        Expr::ArrayFrom(a0, a1) => Expr::ArrayFrom(Shallow::shallow(a0, n), a1.clone()),
        Expr::ArrayFromArrayLikeHoley(a0) => Expr::ArrayFromArrayLikeHoley(Shallow::shallow(a0, n)),
        Expr::IteratorFrom(a0) => Expr::IteratorFrom(Shallow::shallow(a0, n)),
        Expr::TaggedTemplateStrings {
            site_id,
            cooked,
            raw,
        } => Expr::TaggedTemplateStrings {
            site_id: site_id.clone(),
            cooked: Shallow::shallow(cooked, n),
            raw: raw.clone(),
        },
        Expr::TemplateRaw(a0) => Expr::TemplateRaw(Shallow::shallow(a0, n)),
        Expr::IteratorToArray(a0) => Expr::IteratorToArray(Shallow::shallow(a0, n)),
        Expr::GetIterator(a0) => Expr::GetIterator(Shallow::shallow(a0, n)),
        Expr::ArrayIterationPatched => Expr::ArrayIterationPatched,
        Expr::GetAsyncIterator(a0) => Expr::GetAsyncIterator(Shallow::shallow(a0, n)),
        Expr::ForOfToArray(a0) => Expr::ForOfToArray(Shallow::shallow(a0, n)),
        Expr::ForAwaitToArray(a0) => Expr::ForAwaitToArray(Shallow::shallow(a0, n)),
        Expr::ArrayFromMapped {
            iterable,
            map_fn,
            this_arg,
        } => Expr::ArrayFromMapped {
            iterable: Shallow::shallow(iterable, n),
            map_fn: Shallow::shallow(map_fn, n),
            this_arg: Shallow::shallow(this_arg, n),
        },
        Expr::ParseInt { string, radix } => Expr::ParseInt {
            string: Shallow::shallow(string, n),
            radix: Shallow::shallow(radix, n),
        },
        Expr::ParseFloat(a0) => Expr::ParseFloat(Shallow::shallow(a0, n)),
        Expr::NumberCoerce(a0) => Expr::NumberCoerce(Shallow::shallow(a0, n)),
        Expr::BigIntCoerce(a0) => Expr::BigIntCoerce(Shallow::shallow(a0, n)),
        Expr::StringCoerce(a0) => Expr::StringCoerce(Shallow::shallow(a0, n)),
        Expr::TemplateStringCoerce(a0) => Expr::TemplateStringCoerce(Shallow::shallow(a0, n)),
        Expr::ObjectCoerce(a0) => Expr::ObjectCoerce(Shallow::shallow(a0, n)),
        Expr::BooleanCoerce(a0) => Expr::BooleanCoerce(Shallow::shallow(a0, n)),
        Expr::IsNaN(a0) => Expr::IsNaN(Shallow::shallow(a0, n)),
        Expr::IsUndefinedOrBareNan(a0) => Expr::IsUndefinedOrBareNan(Shallow::shallow(a0, n)),
        Expr::IsFinite(a0) => Expr::IsFinite(Shallow::shallow(a0, n)),
        Expr::NumberIsNaN(a0) => Expr::NumberIsNaN(Shallow::shallow(a0, n)),
        Expr::NumberIsFinite(a0) => Expr::NumberIsFinite(Shallow::shallow(a0, n)),
        Expr::NumberIsInteger(a0) => Expr::NumberIsInteger(Shallow::shallow(a0, n)),
        Expr::NumberIsSafeInteger(a0) => Expr::NumberIsSafeInteger(Shallow::shallow(a0, n)),
        Expr::StaticPluginResolve(a0) => Expr::StaticPluginResolve(Shallow::shallow(a0, n)),
        Expr::JsLoadModule { path } => Expr::JsLoadModule { path: path.clone() },
        Expr::JsGetExport {
            module_handle,
            export_name,
        } => Expr::JsGetExport {
            module_handle: Shallow::shallow(module_handle, n),
            export_name: export_name.clone(),
        },
        Expr::JsCallFunction {
            module_handle,
            func_name,
            args,
        } => Expr::JsCallFunction {
            module_handle: Shallow::shallow(module_handle, n),
            func_name: func_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCallMethod {
            object,
            method_name,
            args,
        } => Expr::JsCallMethod {
            object: Shallow::shallow(object, n),
            method_name: method_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCallValue { callee, args } => Expr::JsCallValue {
            callee: Shallow::shallow(callee, n),
            args: Shallow::shallow(args, n),
        },
        Expr::JsGetProperty {
            object,
            property_name,
        } => Expr::JsGetProperty {
            object: Shallow::shallow(object, n),
            property_name: property_name.clone(),
        },
        Expr::JsSetProperty {
            object,
            property_name,
            value,
        } => Expr::JsSetProperty {
            object: Shallow::shallow(object, n),
            property_name: property_name.clone(),
            value: Shallow::shallow(value, n),
        },
        Expr::JsNew {
            module_handle,
            class_name,
            args,
        } => Expr::JsNew {
            module_handle: Shallow::shallow(module_handle, n),
            class_name: class_name.clone(),
            args: Shallow::shallow(args, n),
        },
        Expr::JsNewFromHandle { constructor, args } => Expr::JsNewFromHandle {
            constructor: Shallow::shallow(constructor, n),
            args: Shallow::shallow(args, n),
        },
        Expr::JsCreateCallback {
            closure,
            param_count,
        } => Expr::JsCreateCallback {
            closure: Shallow::shallow(closure, n),
            param_count: param_count.clone(),
        },
        Expr::ImportMetaUrl(a0) => Expr::ImportMetaUrl(a0.clone()),
        Expr::ProxyNew { target, handler } => Expr::ProxyNew {
            target: Shallow::shallow(target, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProxyGet { proxy, key } => Expr::ProxyGet {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxySet { proxy, key, value } => Expr::ProxySet {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
        },
        Expr::ProxyHas { proxy, key } => Expr::ProxyHas {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxyDelete { proxy, key } => Expr::ProxyDelete {
            proxy: Shallow::shallow(proxy, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ProxyApply { proxy, args } => Expr::ProxyApply {
            proxy: Shallow::shallow(proxy, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProxyConstruct { proxy, args } => Expr::ProxyConstruct {
            proxy: Shallow::shallow(proxy, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ProxyRevocable { target, handler } => Expr::ProxyRevocable {
            target: Shallow::shallow(target, n),
            handler: Shallow::shallow(handler, n),
        },
        Expr::ProxyRevoke(a0) => Expr::ProxyRevoke(Shallow::shallow(a0, n)),
        Expr::ReflectGet {
            target,
            key,
            receiver,
        } => Expr::ReflectGet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => Expr::ReflectSet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            receiver: Shallow::shallow(receiver, n),
        },
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict,
            byte_offset,
            byte_end,
        } => Expr::PutValueSet {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            receiver: Shallow::shallow(receiver, n),
            strict: strict.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::ReflectHas { target, key } => Expr::ReflectHas {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ReflectDelete { target, key } => Expr::ReflectDelete {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
        },
        Expr::ReflectOwnKeys(a0) => Expr::ReflectOwnKeys(Shallow::shallow(a0, n)),
        Expr::ReflectApply {
            func,
            this_arg,
            args,
        } => Expr::ReflectApply {
            func: Shallow::shallow(func, n),
            this_arg: Shallow::shallow(this_arg, n),
            args: Shallow::shallow(args, n),
        },
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => Expr::ReflectConstruct {
            target: Shallow::shallow(target, n),
            args: Shallow::shallow(args, n),
            new_target: Shallow::shallow(new_target, n),
        },
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => Expr::ReflectDefineProperty {
            target: Shallow::shallow(target, n),
            key: Shallow::shallow(key, n),
            descriptor: Shallow::shallow(descriptor, n),
        },
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            Expr::ReflectGetOwnPropertyDescriptor {
                target: Shallow::shallow(target, n),
                key: Shallow::shallow(key, n),
            }
        }
        Expr::ReflectGetPrototypeOf(a0) => Expr::ReflectGetPrototypeOf(Shallow::shallow(a0, n)),
        Expr::ReflectSetPrototypeOf { target, proto } => Expr::ReflectSetPrototypeOf {
            target: Shallow::shallow(target, n),
            proto: Shallow::shallow(proto, n),
        },
        Expr::ReflectIsExtensible(a0) => Expr::ReflectIsExtensible(Shallow::shallow(a0, n)),
        Expr::ReflectPreventExtensions(a0) => {
            Expr::ReflectPreventExtensions(Shallow::shallow(a0, n))
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => Expr::ReflectDefineMetadata {
            key: Shallow::shallow(key, n),
            value: Shallow::shallow(value, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectGetMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectGetOwnMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectHasMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectHasOwnMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        } => Expr::ReflectGetMetadataKeys {
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => Expr::ReflectGetOwnMetadataKeys {
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => Expr::ReflectDeleteMetadata {
            key: Shallow::shallow(key, n),
            target: Shallow::shallow(target, n),
            property_key: Shallow::shallow(property_key, n),
        },
        Expr::DynamicImport {
            paths,
            arg,
            options,
            byte_offset,
            deferred_error,
            synchronous,
        } => Expr::DynamicImport {
            paths: paths.clone(),
            arg: Shallow::shallow(arg, n),
            options: Shallow::shallow(options, n),
            byte_offset: byte_offset.clone(),
            deferred_error: deferred_error.clone(),
            synchronous: synchronous.clone(),
        },
        Expr::WorkerNew {
            paths,
            partial,
            filename,
            options,
            is_eval,
        } => Expr::WorkerNew {
            paths: paths.clone(),
            partial: partial.clone(),
            filename: Shallow::shallow(filename, n),
            options: Shallow::shallow(options, n),
            is_eval: is_eval.clone(),
        },
    }
}

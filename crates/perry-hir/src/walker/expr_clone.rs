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
//! `shallow` has an arm per variant, spread over the `shallow_*` modules by
//! the kind of node, each passing the variants it does not own to the next;
//! the last group panics on a variant no group names (the walkers, which have
//! no catch-all, are the compile error for a new variant). A sub-expression field (`Expr`,
//! `Box<Expr>`, `Option<Box<Expr>>`, the lists of them, the expressions of
//! `CallArg`, `ArrayElement` and a parameter's default) is a hole; every
//! other field is cloned. Every copy checks that it made one hole per child
//! the walkers visit and that each visited field is a hole.

use super::*;

mod shallow_collections;
mod shallow_host;
mod shallow_web;

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
        _ => shallow_host::shallow(e, n),
    }
}

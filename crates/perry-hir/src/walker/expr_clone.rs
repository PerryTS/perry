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
//! `shallow` is one exhaustive match with no catch-all (a new variant is a
//! compile error here, as in the walkers): the variants it owns it copies, the
//! rest are or-pattern groups handed to the `shallow_*` modules, whose own
//! matches are `unreachable!` for the other groups' variants. A sub-expression field (`Expr`,
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
            element_offset,
            byte_offset,
            byte_end,
        } => Expr::IndexGet {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            element_offset: element_offset.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::IndexSet {
            object,
            index,
            value,
            element_offset,
            byte_offset,
            byte_end,
        } => Expr::IndexSet {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            value: Shallow::shallow(value, n),
            element_offset: element_offset.clone(),
            byte_offset: byte_offset.clone(),
            byte_end: byte_end.clone(),
        },
        Expr::IndexUpdate {
            object,
            index,
            op,
            prefix,
            strict,
            element_offset,
            byte_offset,
            byte_end,
        } => Expr::IndexUpdate {
            object: Shallow::shallow(object, n),
            index: Shallow::shallow(index, n),
            op: op.clone(),
            prefix: prefix.clone(),
            strict: strict.clone(),
            element_offset: element_offset.clone(),
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
        Expr::ThisProvenClass(class) => Expr::ThisProvenClass(class.clone()),
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
        Expr::ProcessArgv { .. }
        | Expr::ProcessMemoryUsage { .. }
        | Expr::ProcessPid { .. }
        | Expr::ProcessPpid { .. }
        | Expr::ProcessVersion { .. }
        | Expr::ProcessVersions { .. }
        | Expr::ProcessHrtimeBigint { .. }
        | Expr::ProcessHrtime { .. }
        | Expr::ProcessNextTick { .. }
        | Expr::ProcessOn { .. }
        | Expr::ProcessOnce { .. }
        | Expr::ProcessChdir { .. }
        | Expr::ProcessKill { .. }
        | Expr::ProcessExit { .. }
        | Expr::ProcessAbort { .. }
        | Expr::ProcessUmask { .. }
        | Expr::ProcessThreadCpuUsage { .. }
        | Expr::ProcessAvailableMemory { .. }
        | Expr::ProcessConstrainedMemory { .. }
        | Expr::ProcessPosixCredential { .. }
        | Expr::ProcessEmitWarning { .. }
        | Expr::ProcessCpuUsage { .. }
        | Expr::ProcessResourceUsage { .. }
        | Expr::ProcessActiveResourcesInfo { .. }
        | Expr::ProcessTitle { .. }
        | Expr::ProcessSetTitle { .. }
        | Expr::ProcessStdin { .. }
        | Expr::ProcessStdout { .. }
        | Expr::ProcessStderr { .. }
        | Expr::ProcessStdinSetRawMode { .. }
        | Expr::ProcessStdinOn { .. }
        | Expr::ProcessStdinRemoveListener { .. }
        | Expr::ProcessStdinLifecycle { .. }
        | Expr::ProcessStdoutOn { .. }
        | Expr::ProcessStdinIsTTY { .. }
        | Expr::ProcessStdoutIsTTY { .. }
        | Expr::ProcessStderrIsTTY { .. }
        | Expr::ProcessStdoutColumns { .. }
        | Expr::ProcessStdoutRows { .. }
        | Expr::TtyIsAtty { .. }
        | Expr::FsReadFileSync { .. }
        | Expr::FsWriteFileSync { .. }
        | Expr::FsExistsSync { .. }
        | Expr::FsMkdirSync { .. }
        | Expr::FsUnlinkSync { .. }
        | Expr::FsAppendFileSync { .. }
        | Expr::FsReadFileBinary { .. }
        | Expr::FsRmRecursive { .. }
        | Expr::PathJoin { .. }
        | Expr::PathDirname { .. }
        | Expr::PathBasename { .. }
        | Expr::PathBasenameExt { .. }
        | Expr::PathExtname { .. }
        | Expr::PathResolve { .. }
        | Expr::PathIsAbsolute { .. }
        | Expr::PathRelative { .. }
        | Expr::PathNormalize { .. }
        | Expr::PathParse { .. }
        | Expr::PathFormat { .. }
        | Expr::PathSep { .. }
        | Expr::PathDelimiter { .. }
        | Expr::PathToNamespacedPath { .. }
        | Expr::PathMatchesGlob { .. }
        | Expr::PathResolveJoin { .. }
        | Expr::PathWin32Join { .. }
        | Expr::PathWin32 { .. }
        | Expr::WeakRefNew { .. }
        | Expr::WeakRefDeref { .. }
        | Expr::FinalizationRegistryNew { .. }
        | Expr::FinalizationRegistryRegister { .. }
        | Expr::FinalizationRegistryUnregister { .. }
        | Expr::ObjectDefineProperty { .. }
        | Expr::ObjectGetOwnPropertyDescriptor { .. }
        | Expr::ObjectGetOwnPropertyDescriptors { .. }
        | Expr::ObjectGetOwnPropertyNames { .. }
        | Expr::ObjectCreate { .. }
        | Expr::ObjectFreeze { .. }
        | Expr::ObjectSeal { .. }
        | Expr::ObjectPreventExtensions { .. }
        | Expr::ObjectIsFrozen { .. }
        | Expr::ObjectIsSealed { .. }
        | Expr::ObjectIsExtensible { .. }
        | Expr::ObjectGetPrototypeOf { .. }
        | Expr::ObjectSetPrototypeOf { .. }
        | Expr::ObjectDefineProperties { .. }
        | Expr::ObjectGetOwnPropertySymbols { .. }
        | Expr::SymbolNew { .. }
        | Expr::SymbolFor { .. }
        | Expr::SymbolKeyFor { .. }
        | Expr::SymbolDescription { .. }
        | Expr::RegExpEscape { .. }
        | Expr::SymbolToString { .. }
        | Expr::FileURLToPath { .. }
        | Expr::RegExpExecIndex { .. }
        | Expr::RegExpExecGroups { .. }
        | Expr::JsonParse { .. }
        | Expr::JsonParseTyped { .. }
        | Expr::JsonParseReviver { .. }
        | Expr::JsonParseWithReviver { .. }
        | Expr::JsonStringify { .. }
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull { .. }
        | Expr::JsonRawJson { .. }
        | Expr::JsonIsRawJson { .. }
        | Expr::MathFloor { .. }
        | Expr::MathCeil { .. }
        | Expr::MathRound { .. }
        | Expr::MathTrunc { .. }
        | Expr::MathSign { .. }
        | Expr::MathAbs { .. }
        | Expr::MathSqrt { .. }
        | Expr::MathLog { .. }
        | Expr::MathLog2 { .. }
        | Expr::MathLog10 { .. }
        | Expr::MathPow { .. }
        | Expr::MathMin { .. }
        | Expr::MathMax { .. }
        | Expr::MathMinSpread { .. }
        | Expr::MathMaxSpread { .. }
        | Expr::MathImul { .. }
        | Expr::MathRandom { .. }
        | Expr::MathSin { .. }
        | Expr::MathCos { .. }
        | Expr::MathTan { .. }
        | Expr::MathAsin { .. }
        | Expr::MathAcos { .. }
        | Expr::MathAtan { .. }
        | Expr::MathAtan2 { .. }
        | Expr::MathCbrt { .. }
        | Expr::MathHypot { .. }
        | Expr::MathFround { .. }
        | Expr::MathF16round { .. }
        | Expr::MathClz32 { .. }
        | Expr::MathExpm1 { .. }
        | Expr::MathLog1p { .. }
        | Expr::MathSinh { .. }
        | Expr::MathCosh { .. }
        | Expr::MathTanh { .. }
        | Expr::MathAsinh { .. }
        | Expr::MathAcosh { .. }
        | Expr::MathAtanh { .. }
        | Expr::MathExp { .. }
        | Expr::PerformanceNow { .. }
        | Expr::WebAssemblyValidate { .. }
        | Expr::WebAssemblyCompile { .. }
        | Expr::WebAssemblyModuleNew { .. }
        | Expr::WebAssemblyModuleExports { .. }
        | Expr::WebAssemblyModuleImports { .. }
        | Expr::WebAssemblyModuleCustomSections { .. }
        | Expr::WebAssemblyInstantiate { .. }
        | Expr::WebAssemblyCallExport { .. }
        | Expr::Atob { .. }
        | Expr::Btoa { .. }
        | Expr::TextEncoderNew { .. }
        | Expr::TextEncoderEncode { .. }
        | Expr::TextEncoderEncodeInto { .. }
        | Expr::TextDecoderNew { .. }
        | Expr::TextDecoderDecode { .. }
        | Expr::TextDecoderEncoding { .. }
        | Expr::TextDecoderFatal { .. }
        | Expr::TextDecoderIgnoreBom { .. }
        | Expr::EncodeURI { .. }
        | Expr::DecodeURI { .. }
        | Expr::EncodeURIComponent { .. }
        | Expr::DecodeURIComponent { .. }
        | Expr::StructuredClone { .. }
        | Expr::LinkGeneratorPrototype { .. }
        | Expr::QueueMicrotask { .. }
        | Expr::IterResultSet { .. }
        | Expr::IterResultGetValue { .. }
        | Expr::IterResultGetDone { .. }
        | Expr::AsyncStepChain { .. }
        | Expr::AsyncStepResumeNow { .. }
        | Expr::AsyncStepDeclined { .. }
        | Expr::AsyncStepDone { .. }
        | Expr::CurrentStepClosure { .. }
        | Expr::AsyncFirstCall { .. }
        | Expr::AsyncGenResume { .. }
        | Expr::CryptoRandomBytes { .. }
        | Expr::CryptoRandomUUID { .. }
        | Expr::CryptoRandomUUIDv7 { .. }
        | Expr::CryptoSha256 { .. }
        | Expr::CryptoMd5 { .. }
        | Expr::WebCryptoDigest { .. }
        | Expr::WebCryptoImportKey { .. }
        | Expr::WebCryptoExportKey { .. }
        | Expr::WebCryptoSign { .. }
        | Expr::WebCryptoVerify { .. }
        | Expr::WebCryptoDeriveBits { .. }
        | Expr::WebCryptoDeriveKey { .. }
        | Expr::WebCryptoEncrypt { .. }
        | Expr::WebCryptoDecrypt { .. }
        | Expr::WebCryptoGenerateKey { .. }
        | Expr::WebCryptoWrapKey { .. }
        | Expr::WebCryptoUnwrapKey { .. }
        | Expr::CryptoRandomFillSync { .. }
        | Expr::OsPlatform { .. }
        | Expr::OsArch { .. }
        | Expr::OsHostname { .. }
        | Expr::OsHomedir { .. }
        | Expr::OsTmpdir { .. }
        | Expr::OsTotalmem { .. }
        | Expr::OsFreemem { .. }
        | Expr::OsUptime { .. }
        | Expr::OsType { .. }
        | Expr::OsRelease { .. }
        | Expr::OsCpus { .. }
        | Expr::OsNetworkInterfaces { .. }
        | Expr::OsUserInfo { .. }
        | Expr::OsUserInfoBuffer { .. }
        | Expr::OsEOL { .. }
        | Expr::OsDevNull { .. }
        | Expr::OsAvailableParallelism { .. }
        | Expr::OsEndianness { .. }
        | Expr::OsLoadavg { .. }
        | Expr::OsMachine { .. }
        | Expr::OsVersion { .. }
        | Expr::BufferFrom { .. }
        | Expr::BufferFromArrayBuffer { .. }
        | Expr::BufferAlloc { .. }
        | Expr::BufferAllocUnsafe { .. }
        | Expr::BufferConcat { .. }
        | Expr::BufferConcatWithLength { .. }
        | Expr::BufferIsBuffer { .. }
        | Expr::BufferIsEncoding { .. }
        | Expr::BufferByteLength { .. }
        | Expr::BufferToString { .. }
        | Expr::BufferLength { .. }
        | Expr::BufferSlice { .. }
        | Expr::BufferCopy { .. }
        | Expr::BufferWrite { .. }
        | Expr::BufferFill { .. }
        | Expr::BufferEquals { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::BufferIndexSet { .. }
        | Expr::Uint8ArrayNew { .. }
        | Expr::Uint8ArrayFrom { .. }
        | Expr::Uint8ArrayLength { .. }
        | Expr::Uint8ArrayGet { .. }
        | Expr::Uint8ArraySet { .. }
        | Expr::TypedArrayNew { .. }
        | Expr::NativeArenaAlloc { .. }
        | Expr::NativeArenaView { .. }
        | Expr::NativePodView { .. }
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::NativeArenaDispose { .. }
        | Expr::NativeMemoryFillU32 { .. }
        | Expr::NativeMemoryCopy { .. }
        | Expr::ChildProcessExecSync { .. }
        | Expr::ChildProcessSpawnSync { .. }
        | Expr::ChildProcessSpawn { .. }
        | Expr::ChildProcessFork { .. }
        | Expr::ChildProcessExec { .. }
        | Expr::ChildProcessExecFile { .. }
        | Expr::ChildProcessExecFileSync { .. }
        | Expr::ChildProcessSpawnBackground { .. }
        | Expr::ChildProcessGetProcessStatus { .. }
        | Expr::ChildProcessKillProcess { .. } => shallow_host::shallow(e, n),
        Expr::FetchWithOptions { .. }
        | Expr::FetchGetWithAuth { .. }
        | Expr::FetchPostWithAuth { .. }
        | Expr::NetCreateServer { .. }
        | Expr::NetCreateConnection { .. }
        | Expr::NetConnect { .. }
        | Expr::ArrayPush { .. }
        | Expr::ArrayPushSpread { .. }
        | Expr::ArrayPop { .. }
        | Expr::ArrayShift { .. }
        | Expr::ArrayUnshift { .. }
        | Expr::ArrayIndexOf { .. }
        | Expr::ArrayLastIndexOf { .. }
        | Expr::ArrayIncludes { .. }
        | Expr::ArraySlice { .. }
        | Expr::ArraySplice { .. }
        | Expr::ArrayForEach { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFind { .. }
        | Expr::ArrayFindIndex { .. }
        | Expr::ArrayFindLast { .. }
        | Expr::ArrayFindLastIndex { .. }
        | Expr::ArrayAt { .. }
        | Expr::ArraySome { .. }
        | Expr::ArrayEvery { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArraySort { .. }
        | Expr::ArrayReduce { .. }
        | Expr::ArrayReduceRight { .. }
        | Expr::ArrayJoin { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayWith { .. }
        | Expr::ArrayReverseValue { .. }
        | Expr::ArrayCopyWithin { .. }
        | Expr::ArrayCopyWithinValue { .. }
        | Expr::ArrayEntries { .. }
        | Expr::ArrayKeys { .. }
        | Expr::ArrayValues { .. }
        | Expr::ArrayLikeMethod { .. }
        | Expr::StringSplit { .. }
        | Expr::StringFromCharCode { .. }
        | Expr::StringFromCharCodeSpread { .. }
        | Expr::StringFromCodePoint { .. }
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::StringCodePointAt { .. }
        | Expr::MapNew { .. }
        | Expr::MapNewFromArray { .. }
        | Expr::MapSet { .. }
        | Expr::MapGet { .. }
        | Expr::MapHas { .. }
        | Expr::MapDelete { .. }
        | Expr::MapSize { .. }
        | Expr::MapClear { .. }
        | Expr::MapEntries { .. }
        | Expr::MapKeys { .. }
        | Expr::MapValues { .. }
        | Expr::MapEntryKeyAt { .. }
        | Expr::MapEntryValueAt { .. }
        | Expr::SetNew { .. }
        | Expr::SetNewFromArray { .. }
        | Expr::SetAdd { .. }
        | Expr::SetHas { .. }
        | Expr::SetDelete { .. }
        | Expr::SetSize { .. }
        | Expr::SetClear { .. }
        | Expr::SetValues { .. }
        | Expr::SetValueAt { .. }
        | Expr::Sequence { .. }
        | Expr::BoxedPrimitiveNew { .. }
        | Expr::DateNow { .. }
        | Expr::DateNew { .. }
        | Expr::DateGetTime { .. }
        | Expr::DateToISOString { .. }
        | Expr::DateGetFullYear { .. }
        | Expr::DateGetMonth { .. }
        | Expr::DateGetDate { .. }
        | Expr::DateGetDay { .. }
        | Expr::DateGetHours { .. }
        | Expr::DateGetMinutes { .. }
        | Expr::DateGetSeconds { .. }
        | Expr::DateGetMilliseconds { .. }
        | Expr::DateParse { .. }
        | Expr::DateUtc { .. }
        | Expr::DateGetUtcDay { .. }
        | Expr::DateGetUtcFullYear { .. }
        | Expr::DateGetUtcMonth { .. }
        | Expr::DateGetUtcDate { .. }
        | Expr::DateGetUtcHours { .. }
        | Expr::DateGetUtcMinutes { .. }
        | Expr::DateGetUtcSeconds { .. }
        | Expr::DateGetUtcMilliseconds { .. }
        | Expr::DateSetUtcFullYear { .. }
        | Expr::DateSetUtcMonth { .. }
        | Expr::DateSetUtcDate { .. }
        | Expr::DateSetUtcHours { .. }
        | Expr::DateSetUtcMinutes { .. }
        | Expr::DateSetUtcSeconds { .. }
        | Expr::DateSetUtcMilliseconds { .. }
        | Expr::DateSetFullYear { .. }
        | Expr::DateSetMonth { .. }
        | Expr::DateSetDate { .. }
        | Expr::DateSetHours { .. }
        | Expr::DateSetMinutes { .. }
        | Expr::DateSetSeconds { .. }
        | Expr::DateSetMilliseconds { .. }
        | Expr::DateSetTime { .. }
        | Expr::DateValueOf { .. }
        | Expr::DateToString { .. }
        | Expr::DateToDateString { .. }
        | Expr::DateToTimeString { .. }
        | Expr::DateToUTCString { .. }
        | Expr::DateToLocaleDateString { .. }
        | Expr::DateToLocaleTimeString { .. }
        | Expr::DateToLocaleString { .. }
        | Expr::DateGetTimezoneOffset { .. }
        | Expr::DateToJSON { .. }
        | Expr::ErrorNew { .. }
        | Expr::ErrorMessage { .. }
        | Expr::ErrorNewWithCause { .. }
        | Expr::ErrorNewWithOptions { .. }
        | Expr::TypeErrorNew { .. }
        | Expr::RangeErrorNew { .. }
        | Expr::ReferenceErrorNew { .. }
        | Expr::SyntaxErrorNew { .. }
        | Expr::AggregateErrorNew { .. }
        | Expr::UrlNew { .. }
        | Expr::UrlPatternNew { .. } => shallow_collections::shallow(e, n),
        Expr::UrlGetHref { .. }
        | Expr::UrlGetPathname { .. }
        | Expr::UrlGetProtocol { .. }
        | Expr::UrlGetHost { .. }
        | Expr::UrlGetHostname { .. }
        | Expr::UrlGetPort { .. }
        | Expr::UrlGetSearch { .. }
        | Expr::UrlGetHash { .. }
        | Expr::UrlGetOrigin { .. }
        | Expr::UrlGetSearchParams { .. }
        | Expr::UrlCanParse { .. }
        | Expr::UrlCanParseWithBase { .. }
        | Expr::UrlParse { .. }
        | Expr::UrlParseWithBase { .. }
        | Expr::UrlInstanceToString { .. }
        | Expr::UrlInstanceToJSON { .. }
        | Expr::UrlSetPathname { .. }
        | Expr::UrlSetSearch { .. }
        | Expr::UrlSetHash { .. }
        | Expr::UrlSetProtocol { .. }
        | Expr::UrlSetHostname { .. }
        | Expr::UrlSetPort { .. }
        | Expr::UrlSetUsername { .. }
        | Expr::UrlSetPassword { .. }
        | Expr::UrlSetHref { .. }
        | Expr::UrlSearchParamsNew { .. }
        | Expr::UrlSearchParamsMissingArgs { .. }
        | Expr::UrlSearchParamsGet { .. }
        | Expr::UrlSearchParamsHas { .. }
        | Expr::UrlSearchParamsSet { .. }
        | Expr::UrlSearchParamsAppend { .. }
        | Expr::UrlSearchParamsDelete { .. }
        | Expr::UrlSearchParamsToString { .. }
        | Expr::UrlSearchParamsGetAll { .. }
        | Expr::UrlSearchParamsEntries { .. }
        | Expr::UrlSearchParamsKeys { .. }
        | Expr::UrlSearchParamsValues { .. }
        | Expr::UrlSearchParamsSort { .. }
        | Expr::UrlSearchParamsForEach { .. }
        | Expr::Delete { .. }
        | Expr::Closure { .. }
        | Expr::RegExp { .. }
        | Expr::RegExpDynamic { .. }
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. }
        | Expr::StringReplace { .. }
        | Expr::ObjectFromEntries { .. }
        | Expr::ObjectIs { .. }
        | Expr::ObjectHasOwn { .. }
        | Expr::ObjectKeys { .. }
        | Expr::ForInKeys { .. }
        | Expr::ObjectValues { .. }
        | Expr::ObjectEntries { .. }
        | Expr::ObjectGroupBy { .. }
        | Expr::MapGroupBy { .. }
        | Expr::ObjectRest { .. }
        | Expr::ArrayIsArray { .. }
        | Expr::ArrayFrom { .. }
        | Expr::ArrayFromArrayLikeHoley { .. }
        | Expr::IteratorFrom { .. }
        | Expr::TaggedTemplateStrings { .. }
        | Expr::TemplateRaw { .. }
        | Expr::IteratorToArray { .. }
        | Expr::GetIterator { .. }
        | Expr::ArrayIterationPatched { .. }
        | Expr::GetAsyncIterator { .. }
        | Expr::ForOfToArray { .. }
        | Expr::ForAwaitToArray { .. }
        | Expr::ArrayFromMapped { .. }
        | Expr::ParseInt { .. }
        | Expr::ParseFloat { .. }
        | Expr::NumberCoerce { .. }
        | Expr::BigIntCoerce { .. }
        | Expr::StringCoerce { .. }
        | Expr::TemplateStringCoerce { .. }
        | Expr::ObjectCoerce { .. }
        | Expr::BooleanCoerce { .. }
        | Expr::IsNaN { .. }
        | Expr::IsUndefinedOrBareNan { .. }
        | Expr::IsFinite { .. }
        | Expr::NumberIsNaN { .. }
        | Expr::NumberIsFinite { .. }
        | Expr::NumberIsInteger { .. }
        | Expr::NumberIsSafeInteger { .. }
        | Expr::StaticPluginResolve { .. }
        | Expr::JsLoadModule { .. }
        | Expr::JsGetExport { .. }
        | Expr::JsCallFunction { .. }
        | Expr::JsCallMethod { .. }
        | Expr::JsCallValue { .. }
        | Expr::JsGetProperty { .. }
        | Expr::JsSetProperty { .. }
        | Expr::JsNew { .. }
        | Expr::JsNewFromHandle { .. }
        | Expr::JsCreateCallback { .. }
        | Expr::ImportMetaUrl { .. }
        | Expr::ProxyNew { .. }
        | Expr::ProxyGet { .. }
        | Expr::ProxySet { .. }
        | Expr::ProxyHas { .. }
        | Expr::ProxyDelete { .. }
        | Expr::ProxyApply { .. }
        | Expr::ProxyConstruct { .. }
        | Expr::ProxyRevocable { .. }
        | Expr::ProxyRevoke { .. }
        | Expr::ReflectGet { .. }
        | Expr::ReflectSet { .. }
        | Expr::PutValueSet { .. }
        | Expr::ReflectHas { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectOwnKeys { .. }
        | Expr::ReflectApply { .. }
        | Expr::ReflectConstruct { .. }
        | Expr::ReflectDefineProperty { .. }
        | Expr::ReflectGetOwnPropertyDescriptor { .. }
        | Expr::ReflectGetPrototypeOf { .. }
        | Expr::ReflectSetPrototypeOf { .. }
        | Expr::ReflectIsExtensible { .. }
        | Expr::ReflectPreventExtensions { .. }
        | Expr::ReflectDefineMetadata { .. }
        | Expr::ReflectGetMetadata { .. }
        | Expr::ReflectGetOwnMetadata { .. }
        | Expr::ReflectHasMetadata { .. }
        | Expr::ReflectHasOwnMetadata { .. }
        | Expr::ReflectGetMetadataKeys { .. }
        | Expr::ReflectGetOwnMetadataKeys { .. }
        | Expr::ReflectDeleteMetadata { .. }
        | Expr::DynamicImport { .. }
        | Expr::WorkerNew { .. } => shallow_web::shallow(e, n),
    }
}

//! `SH` impls for HIR top-level declarations (classes, interfaces, enums,
//! functions, globals, decorators). Split out of `stable_hash.rs` (no
//! behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

// --- Top-level decls -------------------------------------------------------

impl SH for Class {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Class {
            id,
            name,
            type_params,
            extends,
            extends_name,
            extends_expr,
            native_extends,
            fields,
            constructor,
            methods,
            getters,
            setters,
            static_fields,
            static_methods,
            decorators,
            is_exported,
            aliases,
        } = self;
        id.hash(h);
        name.hash(h);
        type_params.hash(h);
        extends.hash(h);
        extends_name.hash(h);
        extends_expr.hash(h);
        native_extends.hash(h);
        fields.hash(h);
        constructor.hash(h);
        methods.hash(h);
        getters.hash(h);
        setters.hash(h);
        static_fields.hash(h);
        static_methods.hash(h);
        decorators.hash(h);
        is_exported.hash(h);
        aliases.hash(h);
    }
}

impl SH for ClassField {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let ClassField {
            name,
            key_expr,
            ty,
            init,
            is_private,
            is_readonly,
            decorators,
        } = self;
        name.hash(h);
        key_expr.hash(h);
        ty.hash(h);
        init.hash(h);
        is_private.hash(h);
        is_readonly.hash(h);
        decorators.hash(h);
    }
}

impl SH for Interface {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Interface {
            id,
            name,
            type_params,
            extends,
            properties,
            methods,
            is_exported,
        } = self;
        id.hash(h);
        name.hash(h);
        type_params.hash(h);
        extends.hash(h);
        properties.hash(h);
        methods.hash(h);
        is_exported.hash(h);
    }
}

impl SH for InterfaceProperty {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let InterfaceProperty {
            name,
            ty,
            optional,
            readonly,
        } = self;
        name.hash(h);
        ty.hash(h);
        optional.hash(h);
        readonly.hash(h);
    }
}

impl SH for InterfaceMethod {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let InterfaceMethod {
            name,
            type_params,
            params,
            return_type,
        } = self;
        name.hash(h);
        type_params.hash(h);
        params.hash(h);
        return_type.hash(h);
    }
}

impl SH for TypeAlias {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let TypeAlias {
            id,
            name,
            type_params,
            ty,
            is_exported,
        } = self;
        id.hash(h);
        name.hash(h);
        type_params.hash(h);
        ty.hash(h);
        is_exported.hash(h);
    }
}

impl SH for Enum {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Enum {
            id,
            name,
            members,
            is_exported,
        } = self;
        id.hash(h);
        name.hash(h);
        members.hash(h);
        is_exported.hash(h);
    }
}

impl SH for EnumMember {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let EnumMember { name, value } = self;
        name.hash(h);
        value.hash(h);
    }
}

impl SH for EnumValue {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            EnumValue::Number(n) => {
                tag(h, 0);
                n.hash(h);
            }
            EnumValue::String(s) => {
                tag(h, 1);
                s.hash(h);
            }
        }
    }
}

impl SH for Global {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Global {
            id,
            name,
            ty,
            mutable,
            init,
        } = self;
        id.hash(h);
        name.hash(h);
        ty.hash(h);
        mutable.hash(h);
        init.hash(h);
    }
}

impl SH for Decorator {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Decorator {
            name,
            args,
            is_factory,
            is_reflect_metadata,
        } = self;
        name.hash(h);
        args.hash(h);
        is_factory.hash(h);
        is_reflect_metadata.hash(h);
    }
}

impl SH for Function {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Function {
            id,
            name,
            type_params,
            params,
            return_type,
            body,
            is_async,
            is_generator,
            is_exported,
            captures,
            decorators,
            was_plain_async,
            was_unrolled,
        } = self;
        id.hash(h);
        name.hash(h);
        type_params.hash(h);
        params.hash(h);
        return_type.hash(h);
        body.hash(h);
        is_async.hash(h);
        is_generator.hash(h);
        is_exported.hash(h);
        captures.hash(h);
        decorators.hash(h);
        was_plain_async.hash(h);
        was_unrolled.hash(h);
    }
}

impl SH for Param {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let Param {
            id,
            name,
            ty,
            default,
            decorators,
            is_rest,
        } = self;
        id.hash(h);
        name.hash(h);
        ty.hash(h);
        default.hash(h);
        decorators.hash(h);
        is_rest.hash(h);
    }
}

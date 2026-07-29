//! `SH` implementations for HIR type definitions.
//! Split out of `stable_hash.rs` (no behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::types::{FunctionType, ObjectType, PropertyInfo, Type, TypeParam};

// --- HIR type system -------------------------------------------------------

impl SH for Type {
    fn hash<H: StableHasher>(&self, hh: &mut H) {
        match self {
            Type::Void => tag(hh, 0),
            Type::Null => tag(hh, 1),
            Type::Boolean => tag(hh, 2),
            Type::Number => tag(hh, 3),
            Type::Int32 => tag(hh, 4),
            Type::BigInt => tag(hh, 5),
            Type::String => tag(hh, 6),
            Type::StringLiteral(s) => {
                tag(hh, 20);
                s.hash(hh);
            }
            Type::Symbol => tag(hh, 7),
            Type::Array(t) => {
                tag(hh, 8);
                t.as_ref().hash(hh);
            }
            Type::Tuple(ts) => {
                tag(hh, 9);
                ts.hash(hh);
            }
            Type::Object(o) => {
                tag(hh, 10);
                o.hash(hh);
            }
            Type::Function(f) => {
                tag(hh, 11);
                f.hash(hh);
            }
            Type::Union(ts) => {
                tag(hh, 12);
                ts.hash(hh);
            }
            Type::Promise(t) => {
                tag(hh, 13);
                t.as_ref().hash(hh);
            }
            Type::Any => tag(hh, 14),
            Type::Unknown => tag(hh, 15),
            Type::Never => tag(hh, 16),
            Type::Named(s) => {
                tag(hh, 17);
                s.hash(hh);
            }
            Type::TypeVar(s) => {
                tag(hh, 18);
                s.hash(hh);
            }
            Type::Generic { base, type_args } => {
                tag(hh, 19);
                base.hash(hh);
                type_args.hash(hh);
            }
        }
    }
}

impl SH for TypeParam {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let TypeParam {
            name,
            constraint,
            default,
        } = self;
        name.hash(h);
        match constraint {
            None => tag(h, 0),
            Some(t) => {
                tag(h, 1);
                t.as_ref().hash(h);
            }
        }
        match default {
            None => tag(h, 0),
            Some(t) => {
                tag(h, 1);
                t.as_ref().hash(h);
            }
        }
    }
}

impl SH for ObjectType {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let ObjectType {
            name,
            properties,
            property_order,
            index_signature,
        } = self;
        name.hash(h);
        // CRITICAL: properties is a HashMap. Sort by key before emit so
        // insertion order does not leak. This is the entire reason the
        // HIR hash is meaningful as a cache key (#686).
        let mut entries: Vec<(&String, &PropertyInfo)> = properties.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        h.write(&(entries.len() as u64).to_le_bytes());
        for (k, v) in entries {
            k.hash(h);
            v.hash(h);
        }
        match property_order {
            None => tag(h, 0),
            Some(order) => {
                tag(h, 1);
                order.hash(h);
            }
        }
        match index_signature {
            None => tag(h, 0),
            Some(t) => {
                tag(h, 1);
                t.as_ref().hash(h);
            }
        }
    }
}

impl SH for PropertyInfo {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let PropertyInfo {
            ty,
            optional,
            readonly,
        } = self;
        ty.hash(h);
        optional.hash(h);
        readonly.hash(h);
    }
}

impl SH for FunctionType {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let FunctionType {
            params,
            return_type,
            is_async,
            is_generator,
        } = self;
        params.hash(h);
        return_type.as_ref().hash(h);
        is_async.hash(h);
        is_generator.hash(h);
    }
}

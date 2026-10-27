//! An operation certificate, conditional on numeric inputs admitted by the
//! consumer. Parameter annotations alone never certify the array's values.
use crate::{BinaryOp, Expr, Stmt};

/// Conditional ordering: the runtime must prove primitive numeric values or
/// plain own numeric data properties before eliding any callback evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NumericOrder {
    Ascending,
    Descending,
    Keyed { property: String, descending: bool },
}

/// Prove the callback has no observable work for admitted primitive numbers.
/// The whole body must be a numeric difference of the two parameters: no
/// calls, captures, defaults or mutation. Identical one-level property reads
/// are conditional on the consumer proving effect-free own data slots.
/// Keeping this certificate in HIR makes selection independent of JS/TS
/// annotations, source spelling, benchmark names and the callback's code ABI.
pub fn pure_numeric_order(callback: &Expr) -> Option<NumericOrder> {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        captures_new_target,
        is_async,
        is_generator,
        ..
    } = callback
    else {
        return None;
    };
    if params.len() != 2
        || !captures.is_empty()
        || !mutable_captures.is_empty()
        || *captures_this
        || *captures_new_target
        || *is_async
        || *is_generator
        || params.iter().any(|p| {
            p.default.is_some()
                || p.is_rest
                || !p.decorators.is_empty()
                || p.arguments_object.is_some()
        })
        || params[0].id == params[1].id
    {
        return None;
    }
    let [Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Sub,
        left,
        right,
    }))] = body.as_slice()
    else {
        return None;
    };
    let projection = |expr: &Expr| match expr {
        Expr::LocalGet(id) => Some((*id, None)),
        Expr::PropertyGet {
            object, property, ..
        } => match object.as_ref() {
            Expr::LocalGet(id) => Some((*id, Some(property.clone()))),
            _ => None,
        },
        _ => None,
    };
    let (a, key_a) = projection(left)?;
    let (b, key_b) = projection(right)?;
    if key_a != key_b {
        return None;
    }
    let descending = if a == params[0].id && b == params[1].id {
        false
    } else if a == params[1].id && b == params[0].id {
        true
    } else {
        return None;
    };
    match key_a {
        Some(property) => Some(NumericOrder::Keyed {
            property,
            descending,
        }),
        None if descending => Some(NumericOrder::Descending),
        None => Some(NumericOrder::Ascending),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{types::Type, Param};

    pub(super) fn callback() -> Expr {
        Expr::Closure {
            func_id: 1,
            params: [17, 23]
                .into_iter()
                .map(|id| Param {
                    id,
                    name: format!("p{id}"),
                    ty: Type::Any,
                    default: None,
                    decorators: vec![],
                    is_rest: false,
                    arguments_object: None,
                })
                .collect(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Binary {
                op: BinaryOp::Sub,
                left: Box::new(Expr::LocalGet(17)),
                right: Box::new(Expr::LocalGet(23)),
            }))],
            captures: vec![],
            mutable_captures: vec![],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    #[test]
    fn numeric_order_certifies_both_directions_without_type_annotations() {
        let mut c = callback();
        assert_eq!(pure_numeric_order(&c), Some(NumericOrder::Ascending));
        if let Expr::Closure { body, .. } = &mut c {
            if let Stmt::Return(Some(Expr::Binary { left, right, .. })) = &mut body[0] {
                std::mem::swap(left, right);
            }
        }
        assert_eq!(pure_numeric_order(&c), Some(NumericOrder::Descending));
    }

    #[test]
    fn numeric_order_refuses_effects_and_uncertified_projections() {
        let mut c = callback();
        if let Expr::Closure { body, .. } = &mut c {
            body.insert(
                0,
                Stmt::Expr(Expr::LocalSet(9, Box::new(Expr::Number(1.0)))),
            );
        }
        assert_eq!(pure_numeric_order(&c), None);
        let mut c = callback();
        if let Expr::Closure { params, .. } = &mut c {
            params[0].default = Some(Expr::Number(1.0));
        }
        assert_eq!(pure_numeric_order(&c), None);
        let mut c = callback();
        if let Expr::Closure { captures, .. } = &mut c {
            captures.push(7);
        }
        assert_eq!(pure_numeric_order(&c), None);
        let mut c = callback();
        if let Expr::Closure { body, .. } = &mut c {
            if let Stmt::Return(Some(Expr::Binary { left, .. })) = &mut body[0] {
                *left = Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(17)),
                    property: "key".into(),
                });
            }
        }
        assert_eq!(pure_numeric_order(&c), None);
        let mut c = callback();
        if let Expr::Closure { is_async, .. } = &mut c {
            *is_async = true;
        }
        assert_eq!(pure_numeric_order(&c), None);
        assert_eq!(pure_numeric_order(&Expr::LocalGet(1)), None);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    #[test]
    fn same_property_order_is_conditional_but_different_or_effectful_keys_are_refused() {
        let mut callback = super::tests::callback();
        if let Expr::Closure { body, .. } = &mut callback {
            if let Stmt::Return(Some(Expr::Binary { left, right, .. })) = &mut body[0] {
                *left = Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(17)),
                    property: "score".into(),
                });
                *right = Box::new(Expr::PropertyGet {
                    byte_offset: 0,
                    object: Box::new(Expr::LocalGet(23)),
                    property: "score".into(),
                });
            }
        }
        assert_eq!(
            pure_numeric_order(&callback),
            Some(NumericOrder::Keyed {
                property: "score".into(),
                descending: false
            })
        );
        if let Expr::Closure { body, .. } = &mut callback {
            if let Stmt::Return(Some(Expr::Binary { right, .. })) = &mut body[0] {
                if let Expr::PropertyGet { property, .. } = right.as_mut() {
                    *property = "other".into();
                }
            }
        }
        assert_eq!(pure_numeric_order(&callback), None);
    }
}

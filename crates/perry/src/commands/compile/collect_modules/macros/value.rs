//! JSON objects deserialize directly into AST properties in transport order.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
use swc_common::DUMMY_SP;
use swc_ecma_ast as ast;

pub(super) struct Value(pub ast::Expr);

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Value;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON macro value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
                Ok(Value(ast::Expr::Lit(ast::Lit::Null(ast::Null {
                    span: DUMMY_SP,
                }))))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
                Ok(Value(ast::Expr::Lit(ast::Lit::Bool(ast::Bool {
                    span: DUMMY_SP,
                    value,
                }))))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
                self.visit_f64(value as f64)
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
                self.visit_f64(value as f64)
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
                if !value.is_finite() {
                    return Err(E::custom("non-finite JSON macro number"));
                }
                Ok(Value(ast::Expr::Lit(ast::Lit::Num(ast::Number {
                    span: DUMMY_SP,
                    value,
                    raw: None,
                }))))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
                Ok(Value(ast::Expr::Lit(ast::Lit::Str(ast::Str {
                    span: DUMMY_SP,
                    value: value.into(),
                    raw: None,
                }))))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
                self.visit_str(&value)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
                let mut elems = Vec::new();
                while let Some(Value(expr)) = sequence.next_element()? {
                    elems.push(Some(ast::ExprOrSpread {
                        spread: None,
                        expr: Box::new(expr),
                    }));
                }
                Ok(Value(ast::Expr::Array(ast::ArrayLit {
                    span: DUMMY_SP,
                    elems,
                })))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Value, A::Error> {
                let mut props = Vec::new();
                while let Some((key, Value(value))) = object.next_entry::<String, Value>()? {
                    props.push(ast::PropOrSpread::Prop(Box::new(ast::Prop::KeyValue(
                        ast::KeyValueProp {
                            key: ast::PropName::Computed(ast::ComputedPropName {
                                span: DUMMY_SP,
                                expr: Box::new(ast::Expr::Lit(ast::Lit::Str(ast::Str {
                                    span: DUMMY_SP,
                                    value: key.into(),
                                    raw: None,
                                }))),
                            }),
                            value: Box::new(value),
                        },
                    ))));
                }
                Ok(Value(ast::Expr::Object(ast::ObjectLit {
                    span: DUMMY_SP,
                    props,
                })))
            }
        }
        deserializer.deserialize_any(Json)
    }
}

#[derive(Deserialize)]
pub(super) struct Envelope {
    pub ok: bool,
    #[serde(default)]
    pub values: Vec<Value>,
    pub error: Option<String>,
}

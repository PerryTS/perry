//! Building an object literal from its source-ordered properties.
//!
//! `Expr::Object` is a layout: its keys are unique, in first-occurrence
//! order, and its values run in key order. A literal that repeats a key
//! (`{ x: 1, y: 2, x: 3 }`, an enum's reverse map) has one own `x` at the
//! first position holding the last value, while every value still runs in
//! source order. When folding each repeat into its key's slot keeps that
//! order the literal stays a layout; otherwise it is the source-ordered
//! definition sequence `Expr::ObjectSpread`.

use super::Expr;

impl Expr {
    /// The object literal `{ k1: v1, k2: v2, … }` for properties in source
    /// order, keys possibly repeated.
    pub fn object_literal(props: Vec<(String, Expr)>) -> Expr {
        let mut layout: Vec<(String, Expr)> = Vec::with_capacity(props.len());
        let mut slot_of: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::with_capacity(props.len());
        let mut repeated = false;
        for (index, (key, _)) in props.iter().enumerate() {
            if slot_of.contains_key(key.as_str()) {
                repeated = true;
            } else {
                slot_of.insert(key, index);
            }
        }
        if !repeated {
            return Expr::Object(props);
        }
        // A repeat folds into its slot when its value only creates itself (a
        // constant or a function): nothing runs, and nothing that runs between
        // the slot and the repeat can change it. The overwritten value still
        // runs, at the slot.
        let foldable = props
            .iter()
            .enumerate()
            .all(|(index, (key, value))| slot_of[key.as_str()] == index || is_inert(value));
        if !foldable {
            return Expr::ObjectSpread {
                parts: props
                    .into_iter()
                    .map(|(key, value)| (Some(key), value))
                    .collect(),
            };
        }
        let mut slots: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (key, value) in props {
            match slots.get(&key) {
                None => {
                    slots.insert(key.clone(), layout.len());
                    layout.push((key, value));
                }
                Some(&slot) => {
                    let earlier = std::mem::replace(&mut layout[slot].1, Expr::Undefined);
                    layout[slot].1 = if is_inert(&earlier) {
                        value
                    } else {
                        Expr::Sequence(vec![earlier, value])
                    };
                }
            }
        }
        Expr::Object(layout)
    }
}

/// Evaluating the value runs no code and reads nothing that code can change:
/// a constant, or a function (its captures are bindings, not values).
fn is_inert(value: &Expr) -> bool {
    matches!(
        value,
        Expr::Number(_)
            | Expr::Integer(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::Closure { .. }
            | Expr::FuncRef(_)
    )
}

#[cfg(test)]
mod tests {
    use super::Expr;

    fn keys(e: &Expr) -> Vec<&str> {
        match e {
            Expr::Object(props) => props.iter().map(|(k, _)| k.as_str()).collect(),
            other => panic!("not a layout: {other:?}"),
        }
    }

    #[test]
    fn unique_keys_stay_as_written() {
        let lit = Expr::object_literal(vec![
            ("b".to_string(), Expr::LocalGet(1)),
            ("a".to_string(), Expr::Number(2.0)),
        ]);
        assert_eq!(keys(&lit), ["b", "a"]);
    }

    #[test]
    fn an_inert_repeat_folds_into_the_first_slot() {
        let lit = Expr::object_literal(vec![
            ("x".to_string(), Expr::Number(1.0)),
            ("y".to_string(), Expr::LocalGet(1)),
            ("x".to_string(), Expr::String("last".to_string())),
        ]);
        assert_eq!(keys(&lit), ["x", "y"]);
        let Expr::Object(props) = lit else {
            unreachable!()
        };
        assert!(matches!(&props[0].1, Expr::String(s) if s == "last"));
    }

    #[test]
    fn an_overwritten_value_still_runs_at_its_slot() {
        let lit = Expr::object_literal(vec![
            ("x".to_string(), Expr::LocalGet(1)),
            ("x".to_string(), Expr::Number(2.0)),
        ]);
        let Expr::Object(props) = lit else {
            panic!("not a layout")
        };
        assert_eq!(props.len(), 1);
        assert!(matches!(&props[0].1, Expr::Sequence(v)
            if matches!(v[..], [Expr::LocalGet(1), Expr::Number(n)] if n == 2.0)));
    }

    #[test]
    fn a_repeat_that_runs_code_keeps_source_order() {
        let lit = Expr::object_literal(vec![
            ("x".to_string(), Expr::Number(1.0)),
            ("y".to_string(), Expr::LocalGet(1)),
            ("x".to_string(), Expr::LocalGet(2)),
        ]);
        let Expr::ObjectSpread { parts } = lit else {
            panic!("not source-ordered")
        };
        let keys: Vec<_> = parts.iter().map(|(k, _)| k.as_deref()).collect();
        assert_eq!(keys, [Some("x"), Some("y"), Some("x")]);
    }
}

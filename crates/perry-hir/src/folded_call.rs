//! What a folded builtin-method node is a call of.
//!
//! HIR folds `m.get(k)`, `s.has(v)`, `a.push(x)` on a proven receiver into
//! dedicated nodes (`Expr::MapGet`, `Expr::SetHas`, `Expr::ArrayPush`, ...).
//! A user can still shadow the method with an own property or a modified
//! prototype (#10943), and the compiled code then runs that method, so each
//! node in [`folded_call`] IS the call `receiver.method(...args)`: codegen
//! guards it and wpfacts models it from this one table.

use crate::Expr;

/// Where a folded node's receiver comes from.
pub enum Receiver<'a> {
    /// A receiver expression, which may be effectful and is therefore
    /// materialised once.
    Expr(&'a Expr),
    /// A local the fold captured. Re-reading a local is free and cannot be
    /// observed, so it needs no materialisation.
    Local(u32),
}

pub struct FoldedCall<'a> {
    pub receiver: Receiver<'a>,
    /// The name a user property would have to carry to shadow this call.
    pub method: &'static str,
    pub args: Vec<&'a Expr>,
}

/// The folded builtin-method nodes that are calls, and what each one is a
/// call of: codegen guards each (`perry-codegen expr/folded_builtin_override.rs`,
/// `Expr::ArrayPush` through its own slow arms) so an own or inherited user
/// method runs, and wpfacts models each as that method call. A node absent
/// from this table runs its builtin even when an own property shadows it,
/// which is #10943 for that spelling and is why the table is meant to grow
/// to every folded method.
pub fn folded_call(expr: &Expr) -> Option<FoldedCall<'_>> {
    let call = match expr {
        Expr::MapGet { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "get",
            args: vec![key],
        },
        Expr::MapSet { map, key, value } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "set",
            args: vec![key, value],
        },
        Expr::MapHas { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "has",
            args: vec![key],
        },
        Expr::MapDelete { map, key } => FoldedCall {
            receiver: Receiver::Expr(map),
            method: "delete",
            args: vec![key],
        },
        Expr::SetHas { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "has",
            args: vec![value],
        },
        Expr::SetDelete { set, value } => FoldedCall {
            receiver: Receiver::Expr(set),
            method: "delete",
            args: vec![value],
        },
        Expr::SetAdd { set_id, value } => FoldedCall {
            receiver: Receiver::Local(*set_id),
            method: "add",
            args: vec![value],
        },
        // `Expr::ArrayPush` is a call of `push`: codegen does not guard it with
        // the diamond (it would cost the inline store) but honours an own
        // `push` in its slow arms (#11021, `expr/array_push_own.rs`).
        Expr::ArrayPush {
            array_id, value, ..
        } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "push",
            args: vec![value],
        },
        // `a.push()` with NO arguments is not an `ArrayPush`: HIR folds it to
        // this native call so `Set(O, "length", …)` still throws on a frozen
        // array. It is a call on both arms either way, so unlike `ArrayPush`
        // a diamond here costs no inline store (#11021).
        Expr::NativeMethodCall {
            module,
            method,
            object: Some(object),
            args,
            ..
        } if module == "array" && method == "push" && args.is_empty() => FoldedCall {
            receiver: Receiver::Expr(object),
            method: "push",
            args: Vec::new(),
        },
        Expr::ArrayIndexOf {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "indexOf",
            args: match from_index {
                Some(from) => vec![value, from],
                None => vec![value],
            },
        },
        Expr::ArraySlice {
            array, start, end, ..
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "slice",
            args: match end {
                Some(end) => vec![start, end],
                None => vec![start],
            },
        },
        // Date. HIR folds every Date accessor into its own variant, and they
        // reach `js_date_apply_setter` / the getter helpers without passing
        // the property chain at all — which is why `d.setHours = () => 1`
        // emitted no guard and ran the builtin. A getter carries the receiver
        // alone; a setter carries its argument list.
        // #11915: every folded Array method reads `%Array.prototype%`, so it
        // takes the same guard: an own property OR a modified prototype goes
        // to the dispatcher, whose runtime entry performs the real `Get`.
        Expr::ArrayForEach { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "forEach",
            args: vec![callback],
        },
        Expr::ArrayMap { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "map",
            args: vec![callback],
        },
        Expr::ArrayFilter { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "filter",
            args: vec![callback],
        },
        Expr::ArrayFind { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "find",
            args: vec![callback],
        },
        Expr::ArrayFindIndex { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findIndex",
            args: vec![callback],
        },
        Expr::ArrayFindLast { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findLast",
            args: vec![callback],
        },
        Expr::ArrayFindLastIndex { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "findLastIndex",
            args: vec![callback],
        },
        Expr::ArraySome { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "some",
            args: vec![callback],
        },
        Expr::ArrayEvery { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "every",
            args: vec![callback],
        },
        Expr::ArrayFlatMap { array, callback } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "flatMap",
            args: vec![callback],
        },
        Expr::ArraySort { array, comparator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "sort",
            args: vec![comparator],
        },
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "reduce",
            args: std::iter::once(callback.as_ref())
                .chain(initial.as_deref())
                .collect(),
        },
        Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "reduceRight",
            args: std::iter::once(callback.as_ref())
                .chain(initial.as_deref())
                .collect(),
        },
        Expr::ArrayLastIndexOf {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "lastIndexOf",
            args: std::iter::once(value.as_ref())
                .chain(from_index.as_deref())
                .collect(),
        },
        Expr::ArrayIncludes {
            array,
            value,
            from_index,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "includes",
            args: std::iter::once(value.as_ref())
                .chain(from_index.as_deref())
                .collect(),
        },
        Expr::ArrayAt { array, index } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "at",
            args: vec![index],
        },
        Expr::ArrayJoin { array, separator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "join",
            args: separator.as_deref().into_iter().collect(),
        },
        Expr::ArrayFlat { array } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "flat",
            args: Vec::new(),
        },
        Expr::ArrayToReversed { array } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toReversed",
            args: Vec::new(),
        },
        Expr::ArrayToSorted { array, comparator } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toSorted",
            args: comparator.as_deref().into_iter().collect(),
        },
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "toSpliced",
            args: [start.as_ref(), delete_count.as_ref()]
                .into_iter()
                .chain(items.iter())
                .collect(),
        },
        Expr::ArrayWith {
            array,
            index,
            value,
        } => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "with",
            args: vec![index, value],
        },
        Expr::ArrayEntries(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "entries",
            args: Vec::new(),
        },
        Expr::ArrayKeys(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "keys",
            args: Vec::new(),
        },
        Expr::ArrayValues(array) => FoldedCall {
            receiver: Receiver::Expr(array),
            method: "values",
            args: Vec::new(),
        },
        Expr::ArrayShift(array_id) => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "shift",
            args: Vec::new(),
        },
        Expr::ArrayUnshift { array_id, value } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "unshift",
            args: vec![value],
        },
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "splice",
            args: std::iter::once(start.as_ref())
                .chain(delete_count.as_deref())
                .chain(items.iter())
                .collect(),
        },
        Expr::ArrayCopyWithin {
            array_id,
            target,
            start,
            end,
        } => FoldedCall {
            receiver: Receiver::Local(*array_id),
            method: "copyWithin",
            args: [target.as_ref(), start.as_ref()]
                .into_iter()
                .chain(end.as_deref())
                .collect(),
        },
        Expr::DateGetTime(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getTime",
            args: Vec::new(),
        },
        Expr::DateToISOString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toISOString",
            args: Vec::new(),
        },
        Expr::DateGetFullYear(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getFullYear",
            args: Vec::new(),
        },
        Expr::DateGetMonth(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMonth",
            args: Vec::new(),
        },
        Expr::DateGetDate(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getDate",
            args: Vec::new(),
        },
        Expr::DateGetDay(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getDay",
            args: Vec::new(),
        },
        Expr::DateGetHours(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getHours",
            args: Vec::new(),
        },
        Expr::DateGetMinutes(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMinutes",
            args: Vec::new(),
        },
        Expr::DateGetSeconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getSeconds",
            args: Vec::new(),
        },
        Expr::DateGetMilliseconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getMilliseconds",
            args: Vec::new(),
        },
        Expr::DateGetUtcDay(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCDay",
            args: Vec::new(),
        },
        Expr::DateGetUtcFullYear(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCFullYear",
            args: Vec::new(),
        },
        Expr::DateGetUtcMonth(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMonth",
            args: Vec::new(),
        },
        Expr::DateGetUtcDate(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCDate",
            args: Vec::new(),
        },
        Expr::DateGetUtcHours(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCHours",
            args: Vec::new(),
        },
        Expr::DateGetUtcMinutes(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMinutes",
            args: Vec::new(),
        },
        Expr::DateGetUtcSeconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCSeconds",
            args: Vec::new(),
        },
        Expr::DateGetUtcMilliseconds(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getUTCMilliseconds",
            args: Vec::new(),
        },
        // The remaining zero-argument Date folds (#11493). They reach their
        // formatters exactly as the getters above do, and were simply missing
        // from this table: `d.toDateString = () => 1` ran the builtin.
        //
        // `Expr::DateToUTCString` is deliberately ABSENT. HIR folds both
        // `toUTCString` and `toGMTString` into it, so the node does not record
        // which name the source used, and the guard needs that name twice: to
        // test which property the receiver owns, and to dispatch it. Guessing
        // `toUTCString` would make `d.toGMTString()` call an own `toUTCString`,
        // which is wrong in the other direction. Guarding it needs the fold to
        // keep its spelling.
        Expr::DateGetTimezoneOffset(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "getTimezoneOffset",
            args: Vec::new(),
        },
        Expr::DateToJSON(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toJSON",
            args: Vec::new(),
        },
        Expr::DateToDateString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toDateString",
            args: Vec::new(),
        },
        Expr::DateToTimeString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toTimeString",
            args: Vec::new(),
        },
        Expr::DateToLocaleDateString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleDateString",
            args: Vec::new(),
        },
        Expr::DateToLocaleTimeString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleTimeString",
            args: Vec::new(),
        },
        Expr::DateToLocaleString(date) => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "toLocaleString",
            args: Vec::new(),
        },
        Expr::DateSetFullYear { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setFullYear",
            args: args.iter().collect(),
        },
        Expr::DateSetMonth { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMonth",
            args: args.iter().collect(),
        },
        Expr::DateSetDate { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setDate",
            args: args.iter().collect(),
        },
        Expr::DateSetHours { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setHours",
            args: args.iter().collect(),
        },
        Expr::DateSetMinutes { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMinutes",
            args: args.iter().collect(),
        },
        Expr::DateSetSeconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setSeconds",
            args: args.iter().collect(),
        },
        Expr::DateSetMilliseconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setMilliseconds",
            args: args.iter().collect(),
        },
        Expr::DateSetTime { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setTime",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcFullYear { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCFullYear",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMonth { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMonth",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcDate { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCDate",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcHours { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCHours",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMinutes { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMinutes",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcSeconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCSeconds",
            args: args.iter().collect(),
        },
        Expr::DateSetUtcMilliseconds { date, args } => FoldedCall {
            receiver: Receiver::Expr(date),
            method: "setUTCMilliseconds",
            args: args.iter().collect(),
        },
        _ => return None,
    };
    Some(call)
}

#[cfg(test)]
mod tests {
    use super::{folded_call, Receiver};
    use crate::Expr;

    fn lit(n: f64) -> Expr {
        Expr::Number(n)
    }

    /// Every entry answers the three things the diamond needs, and the method
    /// name is the one a user would have to write to shadow the call.
    #[test]
    fn the_table_answers_receiver_method_and_arguments() {
        let map_get = Expr::MapGet {
            map: Box::new(Expr::LocalGet(1)),
            key: Box::new(lit(1.0)),
        };
        let call = folded_call(&map_get).expect("MapGet is a folded `get`");
        assert_eq!(call.method, "get");
        assert_eq!(call.args.len(), 1);
        assert!(matches!(call.receiver, Receiver::Expr(_)));

        let map_set = Expr::MapSet {
            map: Box::new(Expr::LocalGet(1)),
            key: Box::new(lit(1.0)),
            value: Box::new(lit(2.0)),
        };
        let call = folded_call(&map_set).expect("MapSet is a folded `set`");
        assert_eq!((call.method, call.args.len()), ("set", 2));
    }

    /// A fold that captured the receiver as a LOCAL needs no materialisation:
    /// re-reading a local is free and cannot be observed. `ArrayIndexOf`
    /// carries its receiver as an expression, so the local case is pinned on
    /// the node that still has one.
    #[test]
    fn a_local_receiver_is_not_materialized() {
        let indexof = Expr::ArrayIndexOf {
            array: Box::new(Expr::LocalGet(7)),
            value: Box::new(lit(1.0)),
            from_index: None,
        };
        let call = folded_call(&indexof).expect("a folded indexOf");
        assert_eq!(call.method, "indexOf");
        assert!(matches!(call.receiver, Receiver::Expr(Expr::LocalGet(7))));
    }

    /// `push` is a call of `push` on the local array (codegen honours an own
    /// `push` in the node's own slow arms, #11021).
    #[test]
    fn array_push_is_a_call_of_push() {
        let push = Expr::ArrayPush {
            array_id: 7,
            value: Box::new(lit(1.0)),
            field_writeback: None,
        };
        let call = folded_call(&push).expect("ArrayPush is a folded `push`");
        assert_eq!((call.method, call.args.len()), ("push", 1));
        assert!(matches!(call.receiver, Receiver::Local(7)));
    }

    /// A zero-argument `a.push()` is a native call, not an `ArrayPush`, and is
    /// guarded like the other call-only folds (#11021). The single-argument
    /// `push_single` spelling is left alone: it is not this node's shape.
    #[test]
    fn a_zero_argument_push_is_a_folded_guard_target() {
        let zero = Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::LocalGet(7))),
            method: "push".to_string(),
            args: Vec::new(),
        };
        let call = folded_call(&zero).expect("a zero-argument push is guarded");
        assert_eq!((call.method, call.args.len()), ("push", 0));
        assert!(matches!(call.receiver, Receiver::Expr(Expr::LocalGet(7))));
        let single = Expr::NativeMethodCall {
            module: "array".to_string(),
            class_name: None,
            object: Some(Box::new(Expr::LocalGet(7))),
            method: "push_single".to_string(),
            args: vec![lit(1.0)],
        };
        assert!(folded_call(&single).is_none());
    }

    /// An optional argument is part of the call when present and absent when
    /// not — the dispatcher arm must pass exactly what the source passed.
    #[test]
    fn optional_arguments_follow_the_source() {
        let one = Expr::ArraySlice {
            layout: None,
            array: Box::new(Expr::LocalGet(1)),
            start: Box::new(lit(0.0)),
            end: None,
        };
        assert_eq!(folded_call(&one).unwrap().args.len(), 1);
        let two = Expr::ArraySlice {
            layout: None,
            array: Box::new(Expr::LocalGet(1)),
            start: Box::new(lit(0.0)),
            end: Some(Box::new(lit(2.0))),
        };
        assert_eq!(folded_call(&two).unwrap().args.len(), 2);
    }

    /// The zero-argument Date folds missing from the table until #11493, each
    /// under the name a user would write to shadow it.
    #[test]
    fn the_remaining_date_folds_are_guarded_under_their_own_names() {
        let date = || Box::new(Expr::LocalGet(1));
        let cases = [
            (Expr::DateGetTimezoneOffset(date()), "getTimezoneOffset"),
            (Expr::DateToJSON(date()), "toJSON"),
            (Expr::DateToDateString(date()), "toDateString"),
            (Expr::DateToTimeString(date()), "toTimeString"),
            (Expr::DateToLocaleDateString(date()), "toLocaleDateString"),
            (Expr::DateToLocaleTimeString(date()), "toLocaleTimeString"),
            (Expr::DateToLocaleString(date()), "toLocaleString"),
        ];
        for (node, name) in cases {
            let call = folded_call(&node).unwrap_or_else(|| panic!("{name} is guarded"));
            assert_eq!((call.method, call.args.len()), (name, 0));
        }
    }

    /// `DateToUTCString` is both `toUTCString` and `toGMTString`, so the node
    /// cannot name the property to test. Guarding it under either name would
    /// dispatch the other spelling to the wrong own method.
    #[test]
    fn the_shared_utc_gmt_fold_is_not_guarded() {
        let node = Expr::DateToUTCString(Box::new(Expr::LocalGet(1)));
        assert!(folded_call(&node).is_none());
    }

    /// A node that is not a folded builtin method call is left alone: the
    /// guard must not wrap arbitrary expressions in a diamond.
    #[test]
    fn a_non_call_node_is_not_guarded() {
        assert!(folded_call(&lit(1.0)).is_none());
        assert!(folded_call(&Expr::LocalGet(1)).is_none());
        assert!(folded_call(&Expr::MapSize(Box::new(Expr::LocalGet(1)))).is_none());
    }
}

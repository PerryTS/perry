//! The shallow copies of the fetch, array, string, map and date nodes (see [`super::shallow`]).

use super::*;

/// The copy of `e` with a hole (counted in `n`) at each sub-expression, for the
/// variants of this group; any other variant goes on to the next group.
#[allow(clippy::clone_on_copy)]
pub(super) fn shallow(e: &Expr, n: &mut usize) -> Expr {
    match e {
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
        _ => unreachable!("internal compiler error: Expr::clone: not a collections variant"),
    }
}

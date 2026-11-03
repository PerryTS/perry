//! The `Intl.*` constructor natives (split out of `intl.rs` for the
//! file-size gate; unchanged).

use super::*;

pub(crate) extern "C" fn number_format_constructor_thunk(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    make_instance_this(
        closure,
        this,
        KIND_NUMBER,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

pub(crate) extern "C" fn date_time_format_constructor_thunk(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    make_instance_this(
        closure,
        this,
        KIND_DATE_TIME,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

pub(crate) extern "C" fn collator_constructor_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    make_instance(closure, KIND_COLLATOR, rest_arg(rest, 0), rest_arg(rest, 1))
}

pub(crate) extern "C" fn segmenter_constructor_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    require_new_target("Segmenter");
    make_instance(
        closure,
        KIND_SEGMENTER,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

pub(crate) extern "C" fn list_format_constructor_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    require_new_target("ListFormat");
    make_instance(
        closure,
        KIND_LIST_FORMAT,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

pub(crate) extern "C" fn relative_time_format_constructor_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    require_new_target("RelativeTimeFormat");
    make_instance(
        closure,
        KIND_RELATIVE_TIME,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

pub(crate) extern "C" fn plural_rules_constructor_thunk(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    require_new_target("PluralRules");
    make_instance(
        closure,
        KIND_PLURAL_RULES,
        rest_arg(rest, 0),
        rest_arg(rest, 1),
    )
}

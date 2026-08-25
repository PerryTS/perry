use crate::codec::{Charset, Format};
use crate::runtime;
use perry_ffi::{
    js_array_get, js_array_length, throw_with_code, ErrorKind, JsValue, TransientRootScope,
    TransientRootedNanbox,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArrayFormat {
    Brackets,
    Comma,
    Indices,
    Repeat,
}

pub(crate) struct StringifyOptions {
    pub(crate) add_query_prefix: bool,
    pub(crate) allow_dots: bool,
    pub(crate) allow_empty_arrays: bool,
    pub(crate) array_format: ArrayFormat,
    pub(crate) charset: Charset,
    pub(crate) charset_sentinel: bool,
    pub(crate) comma_round_trip: bool,
    pub(crate) delimiter: String,
    pub(crate) encode: bool,
    pub(crate) encode_dot_in_keys: bool,
    pub(crate) encode_values_only: bool,
    pub(crate) format: Format,
    pub(crate) skip_nulls: bool,
    pub(crate) strict_null_handling: bool,
    pub(crate) encoder: Option<TransientRootedNanbox>,
    pub(crate) filter: Option<TransientRootedNanbox>,
    pub(crate) filter_keys: Option<Vec<String>>,
    pub(crate) serialize_date: Option<TransientRootedNanbox>,
    pub(crate) sort: Option<TransientRootedNanbox>,
}

impl Default for StringifyOptions {
    fn default() -> Self {
        Self {
            add_query_prefix: false,
            allow_dots: false,
            allow_empty_arrays: false,
            array_format: ArrayFormat::Indices,
            charset: Charset::Utf8,
            charset_sentinel: false,
            comma_round_trip: false,
            delimiter: "&".to_owned(),
            encode: true,
            encode_dot_in_keys: false,
            encode_values_only: false,
            format: Format::Rfc3986,
            skip_nulls: false,
            strict_null_handling: false,
            encoder: None,
            filter: None,
            filter_keys: None,
            serialize_date: None,
            sort: None,
        }
    }
}

impl StringifyOptions {
    pub(crate) fn from_js(scope: &TransientRootScope, raw: f64) -> Self {
        let mut result = Self::default();
        let value = runtime::from_f64(raw);
        if !value.is_pointer() || runtime::is_closure(value) {
            return result;
        }
        let options = scope.root_nanbox(raw);

        validate_bool(scope, &options, "allowEmptyArrays");
        validate_bool(scope, &options, "encodeDotInKeys");
        validate_bool(scope, &options, "commaRoundTrip");

        result.add_query_prefix = bool_option(scope, &options, "addQueryPrefix", false);
        result.allow_empty_arrays = bool_option(scope, &options, "allowEmptyArrays", false);
        result.charset_sentinel = bool_option(scope, &options, "charsetSentinel", false);
        result.comma_round_trip = bool_option(scope, &options, "commaRoundTrip", false);
        result.encode = bool_option(scope, &options, "encode", true);
        result.encode_dot_in_keys = bool_option(scope, &options, "encodeDotInKeys", false);
        result.encode_values_only = bool_option(scope, &options, "encodeValuesOnly", false);
        result.skip_nulls = bool_option(scope, &options, "skipNulls", false);
        result.strict_null_handling = bool_option(scope, &options, "strictNullHandling", false);

        let allow_dots = field(scope, &options, "allowDots");
        result.allow_dots = if allow_dots.is_undefined() {
            result.encode_dot_in_keys
        } else {
            truthy(allow_dots)
        };

        let delimiter = field(scope, &options, "delimiter");
        if !delimiter.is_undefined() {
            result.delimiter = runtime::owned_string(scope, runtime::as_f64(delimiter));
        }

        let charset = field(scope, &options, "charset");
        if !charset.is_undefined() {
            match runtime::string_value(scope, runtime::as_f64(charset)).as_deref() {
                Some("utf-8") => result.charset = Charset::Utf8,
                Some("iso-8859-1") => result.charset = Charset::Latin1,
                _ => {
                    throw_type("The charset option must be either utf-8, iso-8859-1, or undefined")
                }
            }
        }

        let format = field(scope, &options, "format");
        if !format.is_undefined() {
            match runtime::string_value(scope, runtime::as_f64(format)).as_deref() {
                Some("RFC1738") => result.format = Format::Rfc1738,
                Some("RFC3986") => result.format = Format::Rfc3986,
                _ => throw_type("Unknown format option provided."),
            }
        }

        let array_format = field(scope, &options, "arrayFormat");
        result.array_format =
            match runtime::string_value(scope, runtime::as_f64(array_format)).as_deref() {
                Some("brackets") => ArrayFormat::Brackets,
                Some("comma") => ArrayFormat::Comma,
                Some("repeat") => ArrayFormat::Repeat,
                Some("indices") => ArrayFormat::Indices,
                _ => {
                    let indices = field(scope, &options, "indices");
                    if indices.is_undefined() || truthy(indices) {
                        ArrayFormat::Indices
                    } else {
                        ArrayFormat::Repeat
                    }
                }
            };

        let encoder = field(scope, &options, "encoder");
        if !encoder.is_undefined() && !encoder.is_null() {
            if !runtime::is_closure(encoder) {
                throw_type("Encoder has to be a function.");
            }
            result.encoder = Some(scope.root_nanbox(runtime::as_f64(encoder)));
        }

        let serialize_date = field(scope, &options, "serializeDate");
        if runtime::is_closure(serialize_date) {
            result.serialize_date = Some(scope.root_nanbox(runtime::as_f64(serialize_date)));
        }

        let sort = field(scope, &options, "sort");
        if runtime::is_closure(sort) {
            result.sort = Some(scope.root_nanbox(runtime::as_f64(sort)));
        }

        let filter = field(scope, &options, "filter");
        if runtime::is_closure(filter) {
            result.filter = Some(scope.root_nanbox(runtime::as_f64(filter)));
        } else if runtime::is_array(runtime::as_f64(filter)) {
            let filter = scope.root_nanbox(runtime::as_f64(filter));
            let array = runtime::from_f64(filter.get()).as_pointer();
            let length = unsafe { js_array_length(array) };
            let mut keys = Vec::with_capacity(length as usize);
            for index in 0..length {
                let array = runtime::from_f64(filter.get()).as_pointer();
                let value = unsafe { js_array_get(array, index) };
                if !value.is_undefined() && !value.is_null() {
                    keys.push(runtime::owned_string(scope, runtime::as_f64(value)));
                }
            }
            result.filter_keys = Some(keys);
        }

        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DuplicateMode {
    Combine,
    First,
    Last,
}

pub(crate) struct ParseOptions {
    pub(crate) allow_dots: bool,
    pub(crate) allow_empty_arrays: bool,
    pub(crate) allow_prototypes: bool,
    pub(crate) allow_sparse: bool,
    pub(crate) array_limit: usize,
    pub(crate) charset: Charset,
    pub(crate) charset_sentinel: bool,
    pub(crate) comma: bool,
    pub(crate) decode_dot_in_keys: bool,
    pub(crate) delimiter: String,
    pub(crate) depth: usize,
    pub(crate) duplicates: DuplicateMode,
    pub(crate) ignore_query_prefix: bool,
    pub(crate) interpret_numeric_entities: bool,
    pub(crate) parameter_limit: usize,
    pub(crate) parse_arrays: bool,
    pub(crate) strict_depth: bool,
    pub(crate) strict_null_handling: bool,
    pub(crate) throw_on_limit_exceeded: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            allow_dots: false,
            allow_empty_arrays: false,
            allow_prototypes: false,
            allow_sparse: false,
            array_limit: 20,
            charset: Charset::Utf8,
            charset_sentinel: false,
            comma: false,
            decode_dot_in_keys: false,
            delimiter: "&".to_owned(),
            depth: 5,
            duplicates: DuplicateMode::Combine,
            ignore_query_prefix: false,
            interpret_numeric_entities: false,
            parameter_limit: 1000,
            parse_arrays: true,
            strict_depth: false,
            strict_null_handling: false,
            throw_on_limit_exceeded: false,
        }
    }
}

impl ParseOptions {
    pub(crate) fn from_js(scope: &TransientRootScope, raw: f64) -> Self {
        let mut result = Self::default();
        let value = runtime::from_f64(raw);
        if !value.is_pointer() || runtime::is_closure(value) {
            return result;
        }
        let options = scope.root_nanbox(raw);

        result.allow_dots = bool_option(scope, &options, "allowDots", false);
        result.allow_empty_arrays = bool_option(scope, &options, "allowEmptyArrays", false);
        result.allow_prototypes = bool_option(scope, &options, "allowPrototypes", false);
        result.allow_sparse = bool_option(scope, &options, "allowSparse", false);
        result.charset_sentinel = bool_option(scope, &options, "charsetSentinel", false);
        result.comma = bool_option(scope, &options, "comma", false);
        result.decode_dot_in_keys = bool_option(scope, &options, "decodeDotInKeys", false);
        result.ignore_query_prefix = bool_option(scope, &options, "ignoreQueryPrefix", false);
        result.interpret_numeric_entities =
            bool_option(scope, &options, "interpretNumericEntities", false);
        result.parse_arrays = bool_option(scope, &options, "parseArrays", true);
        result.strict_depth = bool_option(scope, &options, "strictDepth", false);
        result.strict_null_handling = bool_option(scope, &options, "strictNullHandling", false);
        result.throw_on_limit_exceeded =
            bool_option(scope, &options, "throwOnLimitExceeded", false);

        result.array_limit = number_option(scope, &options, "arrayLimit", 20);
        result.depth = number_option(scope, &options, "depth", 5);
        result.parameter_limit = number_option(scope, &options, "parameterLimit", 1000);

        let delimiter = field(scope, &options, "delimiter");
        if !delimiter.is_undefined() {
            if delimiter.is_pointer() && !delimiter.is_any_string() {
                throw_type("Regular-expression delimiters are not supported by the native qs shim");
            }
            result.delimiter = runtime::owned_string(scope, runtime::as_f64(delimiter));
        }

        let charset = field(scope, &options, "charset");
        if !charset.is_undefined() {
            match runtime::string_value(scope, runtime::as_f64(charset)).as_deref() {
                Some("utf-8") => result.charset = Charset::Utf8,
                Some("iso-8859-1") => result.charset = Charset::Latin1,
                _ => {
                    throw_type("The charset option must be either utf-8, iso-8859-1, or undefined")
                }
            }
        }

        let duplicates = field(scope, &options, "duplicates");
        if !duplicates.is_undefined() {
            result.duplicates =
                match runtime::string_value(scope, runtime::as_f64(duplicates)).as_deref() {
                    Some("combine") => DuplicateMode::Combine,
                    Some("first") => DuplicateMode::First,
                    Some("last") => DuplicateMode::Last,
                    _ => throw_type("The duplicates option must be either combine, first, or last"),
                };
        }

        let decoder = field(scope, &options, "decoder");
        if !decoder.is_undefined() && !decoder.is_null() {
            if !runtime::is_closure(decoder) {
                throw_type("Decoder has to be a function.");
            }
            throw_type("Custom decoders are not supported by the native qs shim");
        }

        result
    }
}

fn field(scope: &TransientRootScope, options: &TransientRootedNanbox, name: &str) -> JsValue {
    runtime::field_by_name(scope, options, name)
}

fn bool_option(
    scope: &TransientRootScope,
    options: &TransientRootedNanbox,
    name: &str,
    default: bool,
) -> bool {
    let value = field(scope, options, name);
    if value.is_bool() {
        value.to_bool()
    } else {
        default
    }
}

fn validate_bool(scope: &TransientRootScope, options: &TransientRootedNanbox, name: &str) {
    let value = field(scope, options, name);
    if !value.is_undefined() && !value.is_bool() {
        throw_type(&format!(
            "`{name}` option can only be `true` or `false`, when provided"
        ));
    }
}

fn number_option(
    scope: &TransientRootScope,
    options: &TransientRootedNanbox,
    name: &str,
    default: usize,
) -> usize {
    let value = field(scope, options, name);
    if value.is_number() {
        let value = value.to_number();
        if value.is_finite() && value >= 0.0 {
            return value.floor() as usize;
        }
    }
    default
}

fn truthy(value: JsValue) -> bool {
    if value.is_undefined() || value.is_null() {
        false
    } else if value.is_bool() {
        value.to_bool()
    } else if value.is_number() {
        let number = value.to_number();
        number != 0.0 && !number.is_nan()
    } else {
        true
    }
}

fn throw_type(message: &str) -> ! {
    throw_with_code(message, "", ErrorKind::TypeError)
}

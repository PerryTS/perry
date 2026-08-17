//! Checked value conversions for the public `perry/native` profile.
//!
//! These helpers intentionally return ordinary JS-compatible numbers. The
//! compiler may consume them in an exact native lane (for example while
//! initializing a POD field), but a standalone result remains a JavaScript
//! number. Consequently the 64-bit integer conversions accept only safe
//! integers instead of silently manufacturing an imprecise `number`.

use crate::value::JSValue;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Copy, Debug)]
enum ScalarConversion {
    I32,
    I64,
    U32,
    U64,
    USize,
    F32,
    F64,
}

impl ScalarConversion {
    fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::USize => "usize",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

fn checked_number(value: f64, conversion: ScalarConversion) -> Result<f64, &'static str> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_number() {
        return Err("type");
    }

    let number = js_value.as_number();
    let valid = match conversion {
        ScalarConversion::I32 => integer_in_range(number, i32::MIN as f64, i32::MAX as f64),
        ScalarConversion::I64 => integer_in_range(number, -MAX_SAFE_INTEGER, MAX_SAFE_INTEGER),
        ScalarConversion::U32 => integer_in_range(number, 0.0, u32::MAX as f64),
        ScalarConversion::U64 | ScalarConversion::USize => {
            integer_in_range(number, 0.0, MAX_SAFE_INTEGER)
        }
        ScalarConversion::F32 => number.is_finite() && (number as f32).is_finite(),
        ScalarConversion::F64 => number.is_finite(),
    };
    if !valid {
        return Err("range");
    }

    Ok(match conversion {
        ScalarConversion::F32 => (number as f32) as f64,
        _ => number,
    })
}

fn integer_in_range(number: f64, min: f64, max: f64) -> bool {
    number.is_finite()
        && number >= min
        && number <= max
        && number.trunc() == number
        && !(number == 0.0 && number.is_sign_negative())
}

fn convert_or_throw(value: f64, conversion: ScalarConversion) -> f64 {
    match checked_number(value, conversion) {
        Ok(number) => number,
        Err("type") => crate::collection_iter::throw_type_error(&format!(
            "perry/native {}() expects a number",
            conversion.name()
        )),
        Err(_) => {
            let message = format!(
                "perry/native {}() cannot represent the value exactly",
                conversion.name()
            );
            let string =
                crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
            let error = crate::error::js_rangeerror_new(string);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64))
        }
    }
}

macro_rules! scalar_conversion {
    ($function:ident, $conversion:ident) => {
        #[no_mangle]
        pub extern "C" fn $function(value: f64) -> f64 {
            convert_or_throw(value, ScalarConversion::$conversion)
        }
    };
}

scalar_conversion!(js_perry_native_i32, I32);
scalar_conversion!(js_perry_native_i64, I64);
scalar_conversion!(js_perry_native_u32, U32);
scalar_conversion!(js_perry_native_u64, U64);
scalar_conversion!(js_perry_native_usize, USize);
scalar_conversion!(js_perry_native_f32, F32);
scalar_conversion!(js_perry_native_f64, F64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_conversions_reject_fractional_out_of_range_and_imprecise_numbers() {
        assert_eq!(
            checked_number(-2_147_483_648.0, ScalarConversion::I32),
            Ok(-2_147_483_648.0)
        );
        assert_eq!(
            checked_number(4_294_967_295.0, ScalarConversion::U32),
            Ok(4_294_967_295.0)
        );
        assert!(checked_number(1.5, ScalarConversion::I32).is_err());
        assert!(checked_number(-1.0, ScalarConversion::U32).is_err());
        assert!(checked_number(4_294_967_296.0, ScalarConversion::U32).is_err());
        assert!(checked_number(9_007_199_254_740_992.0, ScalarConversion::U64).is_err());
        assert!(checked_number(-0.0, ScalarConversion::I64).is_err());
    }

    #[test]
    fn float_conversions_are_finite_and_f32_rounds_explicitly() {
        assert_eq!(
            checked_number(0.1, ScalarConversion::F32),
            Ok((0.1_f64 as f32) as f64)
        );
        assert_eq!(checked_number(0.1, ScalarConversion::F64), Ok(0.1));
        assert!(checked_number(f64::NAN, ScalarConversion::F32).is_err());
        assert!(checked_number(f64::INFINITY, ScalarConversion::F64).is_err());
    }
}

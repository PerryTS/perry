// Behavioral parity test for the validator package (perry-stdlib).
//
// All inputs are constants so every assertion is deterministic.

import validator from "validator";

// ── String content predicates ──
console.log("contains:", validator.contains("hello world", "world"));
console.log("equals:", validator.equals("abc", "abc"));
console.log("equals_neg:", validator.equals("abc", "abd"));
console.log("isAlpha:", validator.isAlpha("HelloWorld"));
console.log("isAlpha_neg:", validator.isAlpha("Hello World"));
console.log("isAlphanumeric:", validator.isAlphanumeric("abc123"));
console.log("isAlphanumeric_neg:", validator.isAlphanumeric("abc 123"));
console.log("isEmpty_empty:", validator.isEmpty(""));
console.log("isEmpty_nonempty:", validator.isEmpty("x"));
console.log("isLowercase:", validator.isLowercase("hello"));
console.log("isLowercase_neg:", validator.isLowercase("Hello"));
console.log("isUppercase:", validator.isUppercase("HELLO"));
console.log("isUppercase_neg:", validator.isUppercase("Hello"));

// ── Length ──
console.log("isLength_in_range:", validator.isLength("abcd", { min: 2, max: 6 }));
console.log("isLength_too_short:", validator.isLength("a", { min: 2 }));

// ── Numeric / format predicates ──
console.log("isInt:", validator.isInt("42"));
console.log("isInt_neg:", validator.isInt("4.2"));
console.log("isFloat:", validator.isFloat("3.14"));
console.log("isFloat_neg:", validator.isFloat("hello"));
console.log("isNumeric:", validator.isNumeric("12345"));
console.log("isNumeric_neg:", validator.isNumeric("12.34"));
console.log("isHexadecimal:", validator.isHexadecimal("deadBEEF"));
console.log("isHexadecimal_neg:", validator.isHexadecimal("zzz"));
console.log("isJSON_object:", validator.isJSON('{"a":1}'));
console.log("isJSON_neg:", validator.isJSON("{a:1}"));

// ── Web identifiers ──
console.log("isEmail:", validator.isEmail("user@example.com"));
console.log("isEmail_neg:", validator.isEmail("not-an-email"));
console.log("isURL:", validator.isURL("https://example.com"));
console.log("isURL_neg:", validator.isURL("not a url"));
console.log("isUUID:", validator.isUUID("550e8400-e29b-41d4-a716-446655440000"));
console.log("isUUID_neg:", validator.isUUID("nope"));

/*
@covers
crates/perry-stdlib/src/validator.rs:
  - js_validator_contains
  - js_validator_equals
  - js_validator_is_alpha
  - js_validator_is_alphanumeric
  - js_validator_is_email
  - js_validator_is_empty
  - js_validator_is_float
  - js_validator_is_hexadecimal
  - js_validator_is_int
  - js_validator_is_json
  - js_validator_is_length
  - js_validator_is_length_min
  - js_validator_is_lowercase
  - js_validator_is_numeric
  - js_validator_is_uppercase
  - js_validator_is_url
  - js_validator_is_uuid
*/

// Behavioral parity coverage for strings, regex, and JSON FFI surface.
// All output is deterministic and byte-comparable to Node.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// String basics: length, indexing, charAt, charCodeAt, codePointAt.
const text = "Perry";
line("length", text.length);
line("charAt", text.charAt(2));
line("at-pos", text.at(0));
line("at-neg", text.at(-1));
line("charCodeAt", text.charCodeAt(0));
line("codePointAt", text.codePointAt(0));
line("fromCharCode", String.fromCharCode(72, 105));
line("fromCodePoint", String.fromCodePoint(0x1f600));

// Slice / substring.
line("slice", "abcdef".slice(1, 4));
line("slice-negative", "abcdef".slice(-3));
line("substring", "abcdef".substring(2, 5));

// Search / match.
line("indexOf", "banana".indexOf("an"));
line("indexOf-from", "banana".indexOf("an", 2));
line("lastIndexOf", "banana".lastIndexOf("an"));
line("includes", "banana".includes("nan"));
line("startsWith", "banana".startsWith("ban"));
line("startsWith-at", "banana".startsWith("an", 1));
line("endsWith", "banana".endsWith("ana"));
line("endsWith-at", "banana".endsWith("ban", 3));

// Case + whitespace.
line("toUpper", "perry".toUpperCase());
line("toLower", "PERRY".toLowerCase());
line("trim", "  hi  ".trim());
line("trimStart", "  hi  ".trimStart() + "|");
line("trimEnd", "|" + "  hi  ".trimEnd());

// Pad / repeat / concat.
line("padStart", "5".padStart(3, "0"));
line("padEnd", "5".padEnd(3, "0"));
line("repeat", "ab".repeat(3));
line("concat-method", "hello".concat(" ", "world"));
line("template", `pi=${3.14}, name=${text}`);

// Split / replace / replaceAll.
line("split", "a,b,c".split(",").join("|"));
line("split-n", "a,b,c,d".split(",", 2).join("|"));
line("replace", "abcabc".replace("b", "X"));
line("replaceAll", "abcabc".replaceAll("b", "X"));

// localeCompare / comparison / equality.
line("equals", "abc" === "abc");
line("locale-eq", "abc".localeCompare("abc"));
line("locale-lt", "abc".localeCompare("abd") < 0);

// Normalize / wellFormed.
line("normalize", "é" === "é".normalize());
line("isWellFormed", "perry".isWellFormed());
line("toWellFormed", "abc".toWellFormed());

// btoa / atob.
line("btoa", btoa("hello"));
line("atob", atob("aGVsbG8="));

// Number-to-string variants.
line("toString-radix", (255).toString(16));
line("toFixed", (1.2345).toFixed(2));
line("toExponential", (123456).toExponential(2));
line("toPrecision", (1.2345).toPrecision(3));

// Regex: literal, flags, exec, test, named groups.
const re = /(\d+)-(\d+)/;
const m1 = re.exec("order 42-99 today");
line("exec-full", m1?.[0]);
line("exec-groups", (m1?.[1] ?? "") + "/" + (m1?.[2] ?? ""));
line("exec-index", m1?.index);
line("test-true", /^[a-z]+$/.test("perry"));
line("test-false", /^[a-z]+$/.test("Perry1"));
line("flags", /abc/gi.flags);
line("source", /abc/gi.source);

const reNamed = /(?<year>\d{4})-(?<month>\d{2})/;
const mn = reNamed.exec("2026-05");
line("named-year", mn?.groups?.year);
line("named-month", mn?.groups?.month);

// String + regex methods.
line("match", "find 2 cats and 3 dogs".match(/\d+/g)?.join(","));
const matchAll = Array.from("a1 b2 c3".matchAll(/([a-z])(\d)/g)).map(
  (m) => m[1] + "=" + m[2],
);
line("matchAll", matchAll.join("|"));
line("search", "hello world".search(/world/));
line("replace-regex", "foo bar baz".replace(/ba(.)/g, "X$1"));
line("split-regex", "1, 2,3 ,4".split(/\s*,\s*/).join("|"));

// JSON parse / stringify, primitives, nesting.
line("parse-num", JSON.parse("42"));
line("parse-bool", JSON.parse("true"));
line("parse-null", JSON.parse("null"));
line("parse-str", JSON.parse('"hello"'));
line("parse-arr", JSON.parse("[1,2,3]").join(","));
const parsedObj = JSON.parse('{"a":1,"b":{"c":[1,2]}}');
line("parse-obj-a", parsedObj.a);
line("parse-obj-nested", parsedObj.b.c.join(","));

line("stringify-num", JSON.stringify(42));
line("stringify-str", JSON.stringify("hi"));
line("stringify-bool", JSON.stringify(true));
line("stringify-null", JSON.stringify(null));
line("stringify-arr", JSON.stringify([1, "two", true]));
line(
  "stringify-obj",
  JSON.stringify({ a: 1, b: [2, 3], c: { d: "deep" } }),
);
line("stringify-indent", JSON.stringify({ a: 1, b: 2 }, null, 2));
line(
  "stringify-with-replacer",
  JSON.stringify({ a: 1, b: 2, c: 3 }, ["a", "c"]),
);
const round = JSON.parse(JSON.stringify({ x: [1, 2, { y: "z" }] }));
line("round-trip", round.x[2].y);

// JSON.parse with reviver.
const revived = JSON.parse('{"a":1,"b":2}', (k, v) =>
  typeof v === "number" ? v * 10 : v,
);
line("parse-reviver-a", revived.a);
line("parse-reviver-b", revived.b);

// encode / decode URI components.
line("encodeURIComponent", encodeURIComponent("hello world/é"));
line("decodeURIComponent", decodeURIComponent("hello%20world"));
line("encodeURI", encodeURI("https://example.com/a b"));
line("decodeURI", decodeURI("https://example.com/a%20b"));

console.log("compat-strings-regex-json: ok");

/*
@covers
crates/perry-runtime/src/string.rs:
  - js_atob
  - js_btoa
  - js_get_empty_string
  - js_number_to_exponential
  - js_number_to_fixed
  - js_number_to_precision
  - js_number_to_string
  - js_string_addref
  - js_string_alloc_space
  - js_string_append
  - js_string_at
  - js_string_builder_new
  - js_string_char_at
  - js_string_char_code_at
  - js_string_code_point_at
  - js_string_compare
  - js_string_concat
  - js_string_concat_box
  - js_string_concat_chain
  - js_string_concat_value
  - js_string_ends_with
  - js_string_ends_with_at
  - js_string_equals
  - js_string_from_bytes_longlived
  - js_string_from_bytes_with_capacity
  - js_string_from_char_code
  - js_string_from_code_point
  - js_string_from_wtf8_bytes
  - js_string_index_of
  - js_string_index_of_from
  - js_string_intern
  - js_string_is_well_formed
  - js_string_last_index_of
  - js_string_length
  - js_string_locale_compare
  - js_string_materialize_to_heap
  - js_string_new_sso
  - js_string_normalize
  - js_string_pad_end
  - js_string_pad_start
  - js_string_print
  - js_string_repeat
  - js_string_slice
  - js_string_split
  - js_string_split_n
  - js_string_starts_with
  - js_string_starts_with_at
  - js_string_substring
  - js_string_to_char_array
  - js_string_to_lower_case
  - js_string_to_upper_case
  - js_string_to_well_formed
  - js_string_trim
  - js_string_trim_end
  - js_string_trim_start
  - js_value_concat_string
crates/perry-runtime/src/regex.rs:
  - js_regexp_exec
  - js_regexp_exec_get_groups
  - js_regexp_exec_get_index
  - js_regexp_get_flags
  - js_regexp_get_last_index
  - js_regexp_get_source
  - js_regexp_new
  - js_regexp_set_last_index
  - js_regexp_test
  - js_string_match_all
  - js_string_replace_all_string
  - js_string_replace_regex
  - js_string_replace_regex_named
  - js_string_replace_string
  - js_string_search_regex
  - js_string_split_regex
  - js_string_split_regex_n
crates/perry-runtime/src/json.rs:
  - js_json_get_bool
  - js_json_get_number
  - js_json_get_string
  - js_json_is_valid
  - js_json_parse
  - js_json_parse_typed_array
  - js_json_parse_with_reviver
  - js_json_stringify
  - js_json_stringify_bool
  - js_json_stringify_full
  - js_json_stringify_null
  - js_json_stringify_number
  - js_json_stringify_string
  - js_json_stringify_with_replacer
*/

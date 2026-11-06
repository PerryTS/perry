// Behavioral parity coverage for URL, Date, Math, and numeric coercion FFI.
// Uses fixed timestamps in UTC so the output is deterministic across machines
// and time zones.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// URL parsing / accessors.
const u = new URL("https://user:pw@example.com:8443/a/b?x=1&y=2#frag");
line("href", u.href);
line("protocol", u.protocol);
line("host", u.host);
line("hostname", u.hostname);
line("port", u.port);
line("pathname", u.pathname);
line("search", u.search);
line("hash", u.hash);
line("origin", u.origin);

// URL mutation.
const uu = new URL("https://example.com/old");
uu.pathname = "/new";
uu.search = "?q=perry";
uu.hash = "#section";
line("mut-href", uu.href);

// URL with base.
const rel = new URL("/path?n=1", "https://example.com");
line("with-base", rel.href);
line("canParse-valid", URL.canParse("https://example.com"));
line("canParse-invalid", URL.canParse("not a url"));

// URLSearchParams.
const usp = new URLSearchParams("a=1&b=2&a=3");
line("usp-get", usp.get("a"));
line("usp-getAll", usp.getAll("a").join(","));
line("usp-has", usp.has("b"));
line("usp-size", usp.size);
const uspBuild = new URLSearchParams();
uspBuild.append("k", "1");
uspBuild.append("k", "2");
uspBuild.set("j", "3");
line("usp-built", uspBuild.toString());
line("usp-built-size", uspBuild.size);

// Date — pinned to a known UTC instant.
const fixed = 1704067200000; // 2024-01-01T00:00:00.000Z
const d = new Date(fixed);
line("getTime", d.getTime());
line("toISOString", d.toISOString());
line("toJSON", d.toJSON());
line("UTC-year", d.getUTCFullYear());
line("UTC-month", d.getUTCMonth());
line("UTC-date", d.getUTCDate());
line("UTC-day", d.getUTCDay());
line("UTC-hours", d.getUTCHours());
line("UTC-minutes", d.getUTCMinutes());
line("UTC-seconds", d.getUTCSeconds());
line("UTC-ms", d.getUTCMilliseconds());
line("valueOf", d.valueOf());

const d2 = new Date(fixed);
d2.setUTCFullYear(2025);
d2.setUTCMonth(5); // June
d2.setUTCDate(15);
d2.setUTCHours(12);
d2.setUTCMinutes(30);
d2.setUTCSeconds(45);
d2.setUTCMilliseconds(123);
line("mut-iso", d2.toISOString());

// Date.parse + Date.UTC.
line("Date.parse-iso", Date.parse("2024-01-01T00:00:00.000Z"));
line("Date.UTC", Date.UTC(2024, 0, 1, 0, 0, 0, 0));

// Math constants + numeric methods.
line("PI", Math.PI.toFixed(5));
line("E", Math.E.toFixed(5));
line("LN2", Math.LN2.toFixed(5));
line("LOG2E", Math.LOG2E.toFixed(5));
line("abs", Math.abs(-3.5));
line("floor", Math.floor(3.7));
line("ceil", Math.ceil(3.2));
line("round-half-up", Math.round(0.5));
line("round-half-down", Math.round(-0.5));
line("trunc-pos", Math.trunc(4.9));
line("trunc-neg", Math.trunc(-4.9));
line("sign-pos", Math.sign(7));
line("sign-neg", Math.sign(-7));
line("sign-zero", Math.sign(0));
line("min", Math.min(3, 1, 4, 1, 5));
line("max", Math.max(3, 1, 4, 1, 5));
line("min-spread", Math.min(...[5, 9, 1, 4]));
line("max-spread", Math.max(...[5, 9, 1, 4]));
line("pow", Math.pow(2, 10));
line("sqrt", Math.sqrt(81));
line("cbrt", Math.cbrt(27));
line("hypot", Math.hypot(3, 4));
line("exp", Math.exp(1).toFixed(5));
line("expm1", Math.expm1(1).toFixed(5));
line("log", Math.log(Math.E).toFixed(5));
line("log10", Math.log10(1000));
line("log2", Math.log2(1024));
line("log1p", Math.log1p(0));
line("sin", Math.sin(0));
line("cos", Math.cos(0));
line("tan", Math.tan(0));
line("asin", Math.asin(1).toFixed(5));
line("acos", Math.acos(0).toFixed(5));
line("atan", Math.atan(1).toFixed(5));
line("atan2", Math.atan2(1, 1).toFixed(5));
line("sinh", Math.sinh(0));
line("cosh", Math.cosh(0));
line("tanh", Math.tanh(0));
line("asinh", Math.asinh(0));
line("acosh", Math.acosh(1));
line("atanh", Math.atanh(0));
line("clz32", Math.clz32(1));
line("clz32-zero", Math.clz32(0));
line("imul", Math.imul(7, 6));
line("fround", Math.fround(1.5));

// Number static / parsing.
line("parseInt-bin", parseInt("1010", 2));
line("parseInt-hex", parseInt("ff", 16));
line("parseFloat", parseFloat("3.5abc"));
line("Number-true", Number(true));
line("Number-str", Number("3.14"));
line("Number-empty", Number(""));
line("Number.isFinite", Number.isFinite(1));
line("Number.isFinite-inf", Number.isFinite(Infinity));
line("Number.isNaN", Number.isNaN(NaN));
line("Number.isNaN-num", Number.isNaN(1));
line("Number.isInteger", Number.isInteger(3));
line("Number.isInteger-fp", Number.isInteger(3.5));
line("Number.isSafeInteger", Number.isSafeInteger(2 ** 52));
line("Number.isSafeInteger-unsafe", Number.isSafeInteger(2 ** 53));
line("isFinite-coerce", isFinite("42"));
line("isNaN-coerce", isNaN("foo"));

// BigInt arithmetic.
line("bigint-add", (10n + 20n).toString());
line("bigint-mul", (100n * 100n).toString());
line("bigint-neg", (-5n).toString());
line("bigint-mod", (17n % 5n).toString());
line("bigint-cmp", 5n < 10n);
line("bigint-pow", (2n ** 32n).toString());
line("bigint-from-str", BigInt("12345678901234567890").toString());
line("bigint-radix", BigInt(255).toString(16));
line("bigint-and", (0xffn & 0x0fn).toString(16));
line("bigint-or", (0xf0n | 0x0fn).toString(16));
line("bigint-xor", (0xffn ^ 0x0fn).toString(16));

console.log("compat-url-date-math: ok");

/*
@covers
crates/perry-runtime/src/url.rs:
  - js_url_can_parse
  - js_url_get_hash
  - js_url_get_host
  - js_url_get_hostname
  - js_url_get_href
  - js_url_get_origin
  - js_url_get_pathname
  - js_url_get_port
  - js_url_get_protocol
  - js_url_get_search
  - js_url_get_search_params
  - js_url_new
  - js_url_new_with_base
  - js_url_parse
  - js_url_search_params_append
  - js_url_search_params_delete
  - js_url_search_params_entries_arr
  - js_url_search_params_get
  - js_url_search_params_get_all
  - js_url_search_params_has
  - js_url_search_params_new
  - js_url_search_params_new_any
  - js_url_search_params_new_empty
  - js_url_search_params_set
  - js_url_search_params_size
  - js_url_search_params_to_string
  - js_url_set_hash
  - js_url_set_pathname
  - js_url_set_search
crates/perry-runtime/src/date.rs:
  - js_date_get_date
  - js_date_get_day
  - js_date_get_full_year
  - js_date_get_hours
  - js_date_get_milliseconds
  - js_date_get_minutes
  - js_date_get_month
  - js_date_get_seconds
  - js_date_get_time
  - js_date_get_timezone_offset
  - js_date_get_utc_date
  - js_date_get_utc_day
  - js_date_get_utc_full_year
  - js_date_get_utc_hours
  - js_date_get_utc_milliseconds
  - js_date_get_utc_minutes
  - js_date_get_utc_month
  - js_date_get_utc_seconds
  - js_date_new
  - js_date_new_from_timestamp
  - js_date_new_from_value
  - js_date_parse
  - js_date_set_utc_date
  - js_date_set_utc_full_year
  - js_date_set_utc_hours
  - js_date_set_utc_milliseconds
  - js_date_set_utc_minutes
  - js_date_set_utc_month
  - js_date_set_utc_seconds
  - js_date_to_iso_string
  - js_date_to_json
  - js_date_utc
  - js_date_value_of
crates/perry-runtime/src/math.rs:
  - js_math_acos
  - js_math_acosh
  - js_math_asin
  - js_math_asinh
  - js_math_atan
  - js_math_atan2
  - js_math_atanh
  - js_math_cbrt
  - js_math_clz32
  - js_math_cos
  - js_math_cosh
  - js_math_expm1
  - js_math_fmod
  - js_math_fround
  - js_math_hypot
  - js_math_log
  - js_math_log10
  - js_math_log1p
  - js_math_log2
  - js_math_max_array
  - js_math_min_array
  - js_math_pow
  - js_math_sin
  - js_math_sinh
  - js_math_tan
  - js_math_tanh
crates/perry-runtime/src/bigint.rs:
  - js_bigint_add
  - js_bigint_and
  - js_bigint_cmp
  - js_bigint_div
  - js_bigint_eq
  - js_bigint_from_f64
  - js_bigint_from_i64
  - js_bigint_from_string
  - js_bigint_from_string_radix
  - js_bigint_from_u64
  - js_bigint_is_negative
  - js_bigint_is_zero
  - js_bigint_mod
  - js_bigint_mul
  - js_bigint_neg
  - js_bigint_or
  - js_bigint_pow
  - js_bigint_shl
  - js_bigint_shr
  - js_bigint_sub
  - js_bigint_to_f64
  - js_bigint_to_string
  - js_bigint_to_string_radix
  - js_bigint_xor
*/

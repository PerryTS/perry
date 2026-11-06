// Behavioral parity test for lodash-style helpers exposed via perry-stdlib.
//
// Each block uses fixed inputs so the printed output is deterministic and
// can be diff'd byte-for-byte against `node --experimental-strip-types`
// (which requires the real `lodash` package). Perry resolves the import
// via the bundled `bundled-lodash` shim. When Node lacks the package,
// the parity runner falls back to NODE_FAIL and the file still acts as
// a coverage anchor (see @covers block below).

import _ from "lodash";

// ── Arrays ──
console.log("chunk:", JSON.stringify(_.chunk([1, 2, 3, 4, 5], 2)));
console.log("compact:", JSON.stringify(_.compact([0, 1, false, 2, "", 3, null, NaN, 4, undefined])));
console.log("concat:", JSON.stringify(_.concat([1], [2, 3], 4)));
console.log("difference:", JSON.stringify(_.difference([2, 1, 4, 3], [1, 3])));
console.log("drop:", JSON.stringify(_.drop([1, 2, 3, 4], 2)));
console.log("dropRight:", JSON.stringify(_.dropRight([1, 2, 3, 4], 2)));
console.log("fill:", JSON.stringify(_.fill([1, 2, 3, 4], "*", 1, 3)));
console.log("first:", _.first([1, 2, 3]));
console.log("flatten:", JSON.stringify(_.flatten([1, [2, [3, [4]]]])));
console.log("initial:", JSON.stringify(_.initial([1, 2, 3])));
console.log("last:", _.last([1, 2, 3]));
console.log("reverse:", JSON.stringify(_.reverse([1, 2, 3])));
console.log("tail:", JSON.stringify(_.tail([1, 2, 3, 4])));
console.log("take:", JSON.stringify(_.take([1, 2, 3, 4], 2)));
console.log("takeRight:", JSON.stringify(_.takeRight([1, 2, 3, 4], 2)));
console.log("uniq:", JSON.stringify(_.uniq([1, 2, 2, 3, 3, 3, 4])));

// ── Numbers / collections ──
console.log("clamp:", _.clamp(-10, 0, 5));
console.log("inRange1:", _.inRange(3, 1, 10));
console.log("inRange2:", _.inRange(15, 1, 10));
console.log("range:", JSON.stringify(_.range(0, 5)));
console.log("size_arr:", _.size([1, 2, 3]));
console.log("size_str:", _.size("hello"));
console.log("times:", JSON.stringify(_.times(3, (i: number) => i * 2)));

// ── Strings ──
console.log("camelCase:", _.camelCase("foo bar baz"));
console.log("capitalize:", _.capitalize("hello world"));
console.log("kebabCase:", _.kebabCase("fooBarBaz"));
console.log("snakeCase:", _.snakeCase("fooBarBaz"));
console.log("lowerCase:", _.lowerCase("Foo_Bar-baz"));
console.log("upperCase:", _.upperCase("foo_bar-baz"));
console.log("pad:", _.pad("ab", 6, "-"));
console.log("padStart:", _.padStart("ab", 6, "0"));
console.log("padEnd:", _.padEnd("ab", 6, "."));
console.log("repeat:", _.repeat("ab", 3));
console.log("trim:", _.trim("  hi  "));
console.log("trimStart:", _.trimStart("..hi..", "."));
console.log("trimEnd:", _.trimEnd("..hi..", "."));
console.log("truncate:", _.truncate("the quick brown fox", { length: 10, omission: "…" }));

// ── Predicates ──
console.log("isEmpty_empty:", _.isEmpty([]));
console.log("isEmpty_nonempty:", _.isEmpty([1]));
console.log("isNil_null:", _.isNil(null));
console.log("isNil_undef:", _.isNil(undefined));
console.log("isNil_zero:", _.isNil(0));

// random/random fixed-seed not deterministic — shape only:
const r = _.random(0, 100);
console.log("random_in_range:", r >= 0 && r <= 100);

/*
@covers
crates/perry-stdlib/src/lodash.rs:
  - js_lodash_camel_case
  - js_lodash_capitalize
  - js_lodash_chunk
  - js_lodash_clamp
  - js_lodash_compact
  - js_lodash_concat
  - js_lodash_difference
  - js_lodash_drop
  - js_lodash_drop_right
  - js_lodash_fill
  - js_lodash_first
  - js_lodash_flatten
  - js_lodash_in_range
  - js_lodash_initial
  - js_lodash_is_empty
  - js_lodash_is_nil
  - js_lodash_kebab_case
  - js_lodash_last
  - js_lodash_lower_case
  - js_lodash_pad
  - js_lodash_pad_end
  - js_lodash_pad_start
  - js_lodash_random
  - js_lodash_range
  - js_lodash_repeat
  - js_lodash_reverse
  - js_lodash_size
  - js_lodash_snake_case
  - js_lodash_tail
  - js_lodash_take
  - js_lodash_take_right
  - js_lodash_times
  - js_lodash_trim
  - js_lodash_trim_end
  - js_lodash_trim_start
  - js_lodash_truncate
  - js_lodash_uniq
  - js_lodash_upper_case
*/

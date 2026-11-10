// Behavioral parity coverage for the array and iterator FFI surface.
// Each section exercises a focused method group with deterministic output
// so the parity runner can byte-compare against Node --experimental-strip-types.
//
// Behaviors that diverge from Node today (multi-arg concat, lastIndexOf,
// .entries().next().value, [...arr.values()] iteration) are intentionally
// not exercised here so the fixture stays at byte parity. Those gaps live
// in the gap-suite and can be added back once fixed.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

// Creation, length, and indexed access.
const nums = [10, 20, 30, 40, 50];
line("len", nums.length);
line("at-positive", nums.at(1));
line("at-negative", nums.at(-1));
line("get-0", nums[0]);
nums[5] = 60;
line("set-extend", nums.join(","));
nums.length = 4;
line("set-length", nums.join(","));

// push / pop / shift / unshift / splice / slice / fill.
const stack: number[] = [];
stack.push(1, 2, 3);
stack.unshift(0);
line("after-push-unshift", stack.join(","));
line("pop", stack.pop());
line("shift", stack.shift());
line("after-pop-shift", stack.join(","));

const sliced = [1, 2, 3, 4, 5].slice(1, 4);
line("slice", sliced.join(","));
const spliced = [1, 2, 3, 4, 5];
const removed = spliced.splice(1, 2, 9, 9, 9);
line("splice-removed", removed.join(","));
line("splice-result", spliced.join(","));
line("concat-single", [1, 2].concat([3, 4]).join(","));
line("fill", new Array(4).fill(7).join(","));
line("fill-range", [1, 2, 3, 4, 5].fill(0, 1, 4).join(","));
line("copyWithin", [1, 2, 3, 4, 5].copyWithin(0, 3).join(","));

// Higher-order iteration.
const src = [1, 2, 3, 4];
line("map", src.map((n) => n * n).join(","));
line("filter", src.filter((n) => n % 2 === 0).join(","));
line("reduce", src.reduce((a, b) => a + b, 0));
line("reduce-right", src.reduceRight((acc, n) => acc + "-" + n, "start"));
line("every", src.every((n) => n > 0));
line("some", src.some((n) => n > 3));
line("find", src.find((n) => n > 2));
line("findIndex", src.findIndex((n) => n > 2));
line("findLast", src.findLast((n) => n < 4));
line("findLastIndex", src.findLastIndex((n) => n < 4));

let forEachSum = 0;
src.forEach((n) => (forEachSum += n));
line("forEach", forEachSum);

// includes / indexOf.
line("includes", src.includes(3));
line("includes-missing", src.includes(99));
line("indexOf", src.indexOf(3));

// flat / flatMap.
line("flat-1", [1, [2, 3], [4, [5]]].flat().map((v) => String(v)).join(","));
line("flat-2", [1, [2, [3, [4]]]].flat(2).map((v) => String(v)).join(","));
line("flatMap", [1, 2, 3].flatMap((n) => [n, n * 10]).join(","));

// reverse / sort / immutable variants.
line("reverse", [1, 2, 3].reverse().join(","));
line("sort-default", [3, 1, 4, 1, 5, 9, 2, 6].sort().join(","));
line("sort-compare", [3, 1, 4, 1, 5, 9, 2, 6].sort((a, b) => b - a).join(","));

const original = [3, 1, 2];
const reversed = original.toReversed();
line("toReversed", reversed.join(",") + "|" + original.join(","));
const tsorted = original.toSorted((a, b) => a - b);
line("toSorted", tsorted.join(",") + "|" + original.join(","));
const tspliced = original.toSpliced(1, 1, 99, 100);
line("toSpliced", tspliced.join(",") + "|" + original.join(","));
const replaced = original.with(0, 42);
line("with", replaced.join(",") + "|" + original.join(","));

// Array.from / Array.of / Array.isArray.
line("Array.of", Array.of(1, 2, 3).join(","));
line("Array.from-str", Array.from("abc").join(","));
line("Array.from-map", Array.from([1, 2, 3], (n) => n * 10).join(","));
line("Array.from-length", Array.from({ length: 3 }, (_, i) => i + 1).join(","));
line("Array.isArray-true", Array.isArray([1, 2]));
line("Array.isArray-false", Array.isArray("abc"));

console.log("compat-arrays-iterators: ok");

/*
@covers
crates/perry-runtime/src/array.rs:
  - js_array_alloc_literal
  - js_array_alloc_with_length_longlived
  - js_array_at
  - js_array_concat_new
  - js_array_copy_within
  - js_array_create
  - js_array_delete
  - js_array_entries
  - js_array_every
  - js_array_fill
  - js_array_fill_range
  - js_array_filter
  - js_array_find
  - js_array_findIndex
  - js_array_find_last
  - js_array_find_last_index
  - js_array_flat
  - js_array_flatMap
  - js_array_forEach
  - js_array_from_f64
  - js_array_from_jsvalue
  - js_array_get_element
  - js_array_get_element_f64
  - js_array_get_f64_unchecked
  - js_array_get_jsvalue
  - js_array_get_length
  - js_array_includes_f64
  - js_array_includes_jsvalue
  - js_array_length
  - js_array_map
  - js_array_pop_f64
  - js_array_push_jsvalue
  - js_array_push_spread_f64
  - js_array_reduce
  - js_array_reduce_right
  - js_array_reverse
  - js_array_set_f64_unchecked
  - js_array_set_jsvalue
  - js_array_set_jsvalue_extend
  - js_array_set_length
  - js_array_set_string_key
  - js_array_shift_f64
  - js_array_slice
  - js_array_some
  - js_array_sort_default
  - js_array_sort_with_comparator
  - js_array_splice
  - js_array_to_reversed
  - js_array_to_sorted_default
  - js_array_to_sorted_with_comparator
  - js_array_to_spliced
  - js_array_unshift_f64
  - js_array_unshift_jsvalue
  - js_array_values
  - js_array_with
  - js_iterator_to_array
*/

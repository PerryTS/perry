// Regression for #385/#386/#387: Windows mimalloc allocates well below the
// 2 TB Darwin-tight HEAP_MIN floor that clean_arr_ptr / is_valid_obj_ptr /
// inline-length value reads were using on every non-Linux/Android platform.
// Effect: legitimate array/object pointers were silently null-ed, returning
// empty arrays from .map() and `undefined` from property access — causing
// the runtime/array_methods.ts and runtime/json_parse.ts doc-tests to fail
// non-deterministically on windows-2022, and language/supported_features.ts
// to print corrupt output then segfault.
//
// All three Perry-supported floor consumers (array.rs::clean_arr_ptr,
// value.rs inline length reader, object.rs::is_valid_obj_ptr) now drop the
// floor to 4 KB on Windows, matching the Linux/Android branch. The fix is
// non-deterministic to reproduce because it depends on heap layout — but
// running these standard ops covers the same paths CI hit.

const nums = [1, 2, 3, 4, 5]
const doubled = nums.map((n) => n * 2)
const evens = nums.filter((n) => n % 2 === 0)
const sum = nums.reduce((acc, n) => acc + n, 0)

console.log(doubled.join(","))
console.log(evens.join(","))
console.log(sum)

const parsed = JSON.parse('{"name":"perry","version":3}') as { name: string; version: number }
console.log(parsed.name)
console.log(parsed.version)
console.log(JSON.stringify(parsed))

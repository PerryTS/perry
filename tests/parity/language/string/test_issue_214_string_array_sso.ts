// Regression: issue #214 — `string[]` element access miscompiled when
// the elements were SSO (short-string-optimization) values from
// JSON.parse. Two symptoms shared one root cause:
//
//   1. `arr.indexOf(s)` returned -1 for present elements because the
//      codegen emitted `js_array_indexOf_f64` (raw bit compare); SSO
//      bits never matched a heap-string needle.
//
//   2. `arr[i]` "crashed" with SIGSEGV at a pseudo-random address —
//      really the next consumer (string concat, ===, .toUpperCase())
//      treating the SSO bits as a `*StringHeader` via the inline
//      `bits & POINTER_MASK_I64` unbox pattern. The lower 48 bits of
//      a SHORT_STRING_TAG (0x7FF9) value encode the inline payload,
//      not a heap pointer, so the deref crashed.
//
// Fix: route every "I have a NaN-boxed string handle, need a
// `*StringHeader`" call site through `js_get_string_pointer_unified`,
// which materializes SSO to a real heap StringHeader. Also flip
// `Array.indexOf` (both the dedicated arm and `Expr::ArrayIndexOf`)
// to `js_array_indexOf_jsvalue`, mirroring `includes`.

const nums: number[] = JSON.parse('[1, 2, 3]')
console.log("nums.length=" + nums.length)
console.log("nums[0]=" + nums[0])
console.log("nums.indexOf(2)=" + nums.indexOf(2))

// 5-byte strings → SSO at parse time.
const strs: string[] = JSON.parse('["hello", "world"]')
console.log("strs.length=" + strs.length)
console.log("JSON.stringify(strs)=" + JSON.stringify(strs))

// Bug #1: indexOf of a present element used to return -1.
console.log("strs.indexOf('hello')=" + strs.indexOf("hello"))
console.log("strs.indexOf('world')=" + strs.indexOf("world"))
console.log("strs.indexOf('absent')=" + strs.indexOf("absent"))

// Bug #2: previously SIGSEGV'd in the concat that follows.
console.log("about to access strs[0]")
const e = strs[0]
console.log("strs[0]=" + e)

// Strict equality between SSO array element and heap-string literal.
console.log("e === 'hello': " + (e === "hello"))
console.log("e === 'world': " + (e === "world"))
console.log("e !== 'world': " + (e !== "world"))

// String method dispatch on an SSO receiver.
console.log("e.toUpperCase()=" + e.toUpperCase())
console.log("e.length=" + e.length)
console.log("e.startsWith('he')=" + e.startsWith("he"))
console.log("e.indexOf('ll')=" + e.indexOf("ll"))

// The user's actual perry-pry pattern: string[] of paths, indexOf for
// expansion check.
const expandedNodes: string[] = []
expandedNodes.push("$")
expandedNodes.push("$.foo")
console.log("expanded.indexOf('$')=" + expandedNodes.indexOf("$"))
console.log("expanded.indexOf('$.foo')=" + expandedNodes.indexOf("$.foo"))
console.log("expanded.indexOf('$.bar')=" + expandedNodes.indexOf("$.bar"))

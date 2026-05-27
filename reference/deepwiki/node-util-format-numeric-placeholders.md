# DeepWiki Response

Repository: `nodejs/node`
Question: How does Node.js implement util.format numeric placeholders, specifically the behavior of %d versus %i and %f for string numbers like '12.5', empty strings, negative zero, Infinity, NaN, and BigInt? Please identify the relevant source files, tests, and invariants useful for implementing parity in another runtime.
Search: https://deepwiki.com/search/how-does-nodejs-implement-util_96548efe-c3f7-45cb-bb60-0b3c61fdcf52

**Question**: How does Node.js implement util.format numeric placeholders, specifically the behavior of %d versus %i and %f for string numbers like '12.5', empty strings, negative zero, Infinity, NaN, and BigInt? Please identify the relevant source files, tests, and invariants useful for implementing parity in another runtime.

Node.js's `util.format()` method handles numeric placeholders (`%d`, `%i`, `%f`) by converting arguments to numbers using JavaScript's built-in type conversion rules, with specific behaviors for different input types. The core logic for `util.format` is defined in `doc/api/util.md` [1](#ref-q1-1)  and tested extensively in `test/parallel/test-util-format.js` [2](#ref-q1-2) .

### Numeric Placeholder Behavior

*   **`%d` (Number)**: This specifier attempts to convert values to a `Number` type.
    *   **String Numbers ('12.5')**: Converts to `12.5` [3](#ref-q1-3) .
    *   **Empty Strings (`''`)**: Converts to `0` [4](#ref-q1-4) .
    *   **Negative Zero (`-0`)**: Converts to `'-0'` [5](#ref-q1-5) .
    *   **Infinity/`-Infinity`**: Converts to `'Infinity'` or `'-Infinity'` [6](#ref-q1-6) .
    *   **`NaN`**: If the input is a `Symbol`, it converts to `'NaN'` [7](#ref-q1-7) .
    *   **`BigInt`**: `BigInt` values are represented with an `'n'` suffix, e.g., `'1180591620717411303424n'` [8](#ref-q1-8) . This behavior was added in Node.js v10.12.0 [9](#ref-q1-9) .

*   **`%i` (Integer)**: This specifier uses `parseInt(value, 10)` for conversion.
    *   **String Numbers ('12.5')**: Converts to `12` [10](#ref-q1-10) .
    *   **Empty Strings (`''`)**: Converts to `'NaN'` [11](#ref-q1-11) .
    *   **Negative Zero (`-0`)**: Converts to `'-0'` [12](#ref-q1-12) .
    *   **Infinity/`-Infinity`**: Converts to `'NaN'` [13](#ref-q1-13) .
    *   **`NaN`**: If the input is a `Symbol`, it converts to `'NaN'` [14](#ref-q1-14) .
    *   **`BigInt`**: `BigInt` values are represented with an `'n'` suffix, similar to `%d` [15](#ref-q1-15) .

*   **`%f` (Float)**: This specifier uses `parseFloat(value)` for conversion.
    *   **String Numbers ('12.5')**: Converts to `1.5` [16](#ref-q1-16) .
    *   **Empty Strings (`''`)**: Converts to `'NaN'` [17](#ref-q1-17) .
    *   **Negative Zero (`-0`)**: Converts to `'-0'` [18](#ref-q1-18) .
    *   **Infinity/`-Infinity`**: Converts to `'Infinity'` or `'-Infinity'` [19](#ref-q1-19) .
    *   **`NaN`**: If the input is a `Symbol`, it converts to `'NaN'` [20](#ref-q1-20) .
    *   **`BigInt`**: Converts to its numeric string representation without the `'n'` suffix, e.g., `5n` becomes `'5'` [21](#ref-q1-21) .

### Relevant Source Files

The primary implementation of `util.format` is in `lib/util.js` [22](#ref-q1-22) . The documentation for its behavior is in `doc/api/util.md` [1](#ref-q1-1) . The underlying number conversion logic for `parseInt` and `parseFloat` is handled by V8, with relevant C++ files like `deps/v8/src/runtime/runtime-numbers.cc` [23](#ref-q1-23)  [24](#ref-q1-24)  and `deps/v8/src/numbers/conversions.cc` [25](#ref-q1-25)  [26](#ref-q1-26)  defining how strings are converted to numbers and how numbers are converted to strings.

### Tests

The behavior of `util.format` for various numeric inputs is thoroughly tested in `test/parallel/test-util-format.js` [2](#ref-q1-2) . This file contains specific assertions for each placeholder (`%d`, `%i`, `%f`, `%s`) and various edge cases like empty strings, negative zero, `Infinity`, `NaN`, and `BigInt` values.

### Invariants for Parity Implementation

To implement parity in another runtime, the following invariants should be maintained:

1.  **`%d` and `%i` with `BigInt`**: `BigInt` values should be formatted with an `'n'` suffix [27](#ref-q1-27)  [15](#ref-q1-15) .
2.  **`%f` with `BigInt`**: `BigInt` values should be converted to their numeric string representation without the `'n'` suffix [21](#ref-q1-21) .
3.  **`%d` and `%f` with empty string**: An empty string `''` should convert to `0` for `%d` [4](#ref-q1-4)  and `'NaN'` for `%f` [17](#ref-q1-17) .
4.  **`%i` with empty string**: An empty string `''` should convert to `'NaN'` for `%i` [11](#ref-q1-11) .
5.  **Negative Zero**: `-0` should be formatted as `'-0'` for `%d`, `%i`, and `%f` [5](#ref-q1-5)  [12](#ref-q1-12)  [18](#ref-q1-18) .
6.  **`Infinity` and `-Infinity`**: These should be formatted as `'Infinity'` and `'-Infinity'` respectively for `%d` and `%f` [6](#ref-q1-6)  [19](#ref-q1-19) . For `%i`, they should be `'NaN'` [13](#ref-q1-13) .
7.  **`Symbol` values**: `Symbol` values should result in `'NaN'` for `%d`, `%i`, and `%f` [7](#ref-q1-7)  [14](#ref-q1-14)  [20](#ref-q1-20) .
8.  **String to Number Conversion**: The behavior of `parseInt(value, 10)` for `%i` and `parseFloat(value)` for `%f` should align with JavaScript's standard library functions.

## Notes

The `util.formatWithOptions` function also supports a `numericSeparator` option, which can affect the output string for numbers and BigInts by adding underscores as separators [28](#ref-q1-28)  [29](#ref-q1-29) . This is primarily for `util.inspect` and `util.formatWithOptions`, not directly for `util.format`'s default behavior.

Wiki pages you might want to explore:
- [Node.js Overview (nodejs/node)](/wiki/nodejs/node#1)

View this search on DeepWiki: https://deepwiki.com/search/how-does-nodejs-implement-util_96548efe-c3f7-45cb-bb60-0b3c61fdcf52

## References

<a id="ref-q1-1"></a>
### [1] `doc/api/util.md:436-540`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/util.md#L436-L540)

```markdown
## `util.format(format[, ...args])`

<!-- YAML
added: v0.5.3
changes:
  - version: v12.11.0
    pr-url: https://github.com/nodejs/node/pull/29606
    description: The `%c` specifier is ignored now.
  - version: v12.0.0
    pr-url: https://github.com/nodejs/node/pull/23162
    description: The `format` argument is now only taken as such if it actually
                 contains format specifiers.
  - version: v12.0.0
    pr-url: https://github.com/nodejs/node/pull/23162
    description: If the `format` argument is not a format string, the output
                 string's formatting is no longer dependent on the type of the
                 first argument. This change removes previously present quotes
                 from strings that were being output when the first argument
                 was not a string.
  - version: v11.4.0
    pr-url: https://github.com/nodejs/node/pull/23708
    description: The `%d`, `%f`, and `%i` specifiers now support Symbols
                 properly.
  - version: v11.4.0
    pr-url: https://github.com/nodejs/node/pull/24806
    description: The `%o` specifier's `depth` has default depth of 4 again.
  - version: v11.0.0
    pr-url: https://github.com/nodejs/node/pull/17907
    description: The `%o` specifier's `depth` option will now fall back to the
                 default depth.
  - version: v10.12.0
    pr-url: https://github.com/nodejs/node/pull/22097
    description: The `%d` and `%i` specifiers now support BigInt.
  - version: v8.4.0
    pr-url: https://github.com/nodejs/node/pull/14558
    description: The `%o` and `%O` specifiers are supported now.
-->

* `format` {string} A `printf`-like format string.

The `util.format()` method returns a formatted string using the first argument
as a `printf`-like format string which can contain zero or more format
specifiers. Each specifier is replaced with the converted value from the
corresponding argument. Supported specifiers are:

* `%s`: `String` will be used to convert all values except `BigInt`, `Object`
  and `-0`. `BigInt` values will be represented with an `n` and Objects that
  have neither a user defined `toString` function nor `Symbol.toPrimitive` function are inspected using `util.inspect()`
  with options `{ depth: 0, colors: false, compact: 3 }`.
* `%d`: `Number` will be used to convert all values except `BigInt` and
  `Symbol`.
* `%i`: `parseInt(value, 10)` is used for all values except `BigInt` and
  `Symbol`.
* `%f`: `parseFloat(value)` is used for all values expect `Symbol`.
* `%j`: JSON. Replaced with the string `'[Circular]'` if the argument contains
  circular references.
* `%o`: `Object`. A string representation of an object with generic JavaScript
  object formatting. Similar to `util.inspect()` with options
  `{ showHidden: true, showProxy: true }`. This will show the full object
  including non-enumerable properties and proxies.
* `%O`: `Object`. A string representation of an object with generic JavaScript
  object formatting. Similar to `util.inspect()` without options. This will show
  the full object not including non-enumerable properties and proxies.
* `%c`: `CSS`. This specifier is ignored and will skip any CSS passed in.
* `%%`: single percent sign (`'%'`). This does not consume an argument.
* Returns: {string} The formatted string

If a specifier does not have a corresponding argument, it is not replaced:

```js
util.format('%s:%s', 'foo');
// Returns: 'foo:%s'
```

Values that are not part of the format string are formatted using
`util.inspect()` if their type is not `string`.

If there are more arguments passed to the `util.format()` method than the
number of specifiers, the extra arguments are concatenated to the returned
string, separated by spaces:

```js
util.format('%s:%s', 'foo', 'bar', 'baz');
// Returns: 'foo:bar baz'
```

If the first argument does not contain a valid format specifier, `util.format()`
returns a string that is the concatenation of all arguments separated by spaces:

```js
util.format(1, 2, 3);
// Returns: '1 2 3'
```

If only one argument is passed to `util.format()`, it is returned as it is
without any formatting:

```js
util.format('%% %s');
// Returns: '%% %s'
```

`util.format()` is a synchronous method that is intended as a debugging tool.
Some input values can have a significant performance overhead that can block the
event loop. Use this function with care and never in a hot code path.
```

<a id="ref-q1-2"></a>
### [2] `test/parallel/test-util-format.js:1-193`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L1-L193)

```javascript
// Copyright Joyent, Inc. and other Node contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

'use strict';
require('../common');
const assert = require('assert');
const util = require('util');
const symbol = Symbol('foo');

assert.strictEqual(util.format(), '');
assert.strictEqual(util.format(''), '');
assert.strictEqual(util.format([]), '[]');
assert.strictEqual(util.format([0]), '[ 0 ]');
assert.strictEqual(util.format({}), '{}');
assert.strictEqual(util.format({ foo: 42 }), '{ foo: 42 }');
assert.strictEqual(util.format(null), 'null');
assert.strictEqual(util.format(true), 'true');
assert.strictEqual(util.format(false), 'false');
assert.strictEqual(util.format('test'), 'test');

// CHECKME this is for console.log() compatibility - but is it *right*?
assert.strictEqual(util.format('foo', 'bar', 'baz'), 'foo bar baz');

// ES6 Symbol handling
assert.strictEqual(util.format(symbol), 'Symbol(foo)');
assert.strictEqual(util.format('foo', symbol), 'foo Symbol(foo)');
assert.strictEqual(util.format('%s', symbol), 'Symbol(foo)');
assert.strictEqual(util.format('%j', symbol), 'undefined');

// Number format specifier
assert.strictEqual(util.format('%d'), '%d');
assert.strictEqual(util.format('%d', 42.0), '42');
assert.strictEqual(util.format('%d', 42), '42');
assert.strictEqual(util.format('%d', '42'), '42');
assert.strictEqual(util.format('%d', '42.0'), '42');
assert.strictEqual(util.format('%d', 1.5), '1.5');
assert.strictEqual(util.format('%d', -0.5), '-0.5');
assert.strictEqual(util.format('%d', -0.0), '-0');
assert.strictEqual(util.format('%d', ''), '0');
assert.strictEqual(util.format('%d', ' -0.000'), '-0');
assert.strictEqual(util.format('%d', Symbol()), 'NaN');
assert.strictEqual(util.format('%d', Infinity), 'Infinity');
assert.strictEqual(util.format('%d', -Infinity), '-Infinity');
assert.strictEqual(util.format('%d %d', 42, 43), '42 43');
assert.strictEqual(util.format('%d %d', 42), '42 %d');
assert.strictEqual(
  util.format('%d', 1180591620717411303424),
  '1.1805916207174113e+21'
);
assert.strictEqual(
  util.format('%d', 1180591620717411303424n),
  '1180591620717411303424n'
);
assert.strictEqual(
  util.format('%d %d', 1180591620717411303424n, 12345678901234567890123n),
  '1180591620717411303424n 12345678901234567890123n'
);

{
  const { numericSeparator } = util.inspect.defaultOptions;
  util.inspect.defaultOptions.numericSeparator = true;

  assert.strictEqual(
    util.format('%d', 1180591620717411303424),
    '1.1805916207174113e+21'
  );

  assert.strictEqual(
    util.format(
      // eslint-disable-next-line no-loss-of-precision
      '%d %s %i', 118059162071741130342, 118059162071741130342, 123_123_123),
    '118_059_162_071_741_140_000 118_059_162_071_741_140_000 123_123_123'
  );

  assert.strictEqual(
    util.format(
      '%d %s',
      1_180_591_620_717_411_303_424n,
      12_345_678_901_234_567_890_123n
    ),
    '1_180_591_620_717_411_303_424n 12_345_678_901_234_567_890_123n'
  );

  assert.strictEqual(
    util.format('%i', 1_180_591_620_717_411_303_424n),
    '1_180_591_620_717_411_303_424n'
  );

  util.inspect.defaultOptions.numericSeparator = numericSeparator;
}
// Integer format specifier
assert.strictEqual(util.format('%i'), '%i');
assert.strictEqual(util.format('%i', 42.0), '42');
assert.strictEqual(util.format('%i', 42), '42');
assert.strictEqual(util.format('%i', '42'), '42');
assert.strictEqual(util.format('%i', '42.0'), '42');
assert.strictEqual(util.format('%i', 1.5), '1');
assert.strictEqual(util.format('%i', -0.5), '-0');
assert.strictEqual(util.format('%i', ''), 'NaN');
assert.strictEqual(util.format('%i', Infinity), 'NaN');
assert.strictEqual(util.format('%i', -Infinity), 'NaN');
assert.strictEqual(util.format('%i', Symbol()), 'NaN');
assert.strictEqual(util.format('%i %i', 42, 43), '42 43');
assert.strictEqual(util.format('%i %i', 42), '42 %i');
assert.strictEqual(
  util.format('%i', 1180591620717411303424),
  '1'
);
assert.strictEqual(
  util.format('%i', 1180591620717411303424n),
  '1180591620717411303424n'
);
assert.strictEqual(
  util.format('%i %i', 1180591620717411303424n, 12345678901234567890123n),
  '1180591620717411303424n 12345678901234567890123n'
);

assert.strictEqual(
  util.format('%d %i', 1180591620717411303424n, 12345678901234567890123n),
  '1180591620717411303424n 12345678901234567890123n'
);

assert.strictEqual(
  util.format('%i %d', 1180591620717411303424n, 12345678901234567890123n),
  '1180591620717411303424n 12345678901234567890123n'
);

assert.strictEqual(
  util.formatWithOptions(
    { numericSeparator: true },
    '%i %d', 1180591620717411303424n, 12345678901234567890123n),
  '1_180_591_620_717_411_303_424n 12_345_678_901_234_567_890_123n'
);

// Float format specifier
assert.strictEqual(util.format('%f'), '%f');
assert.strictEqual(util.format('%f', 42.0), '42');
assert.strictEqual(util.format('%f', 42), '42');
assert.strictEqual(util.format('%f', '42'), '42');
assert.strictEqual(util.format('%f', '-0.0'), '-0');
assert.strictEqual(util.format('%f', '42.0'), '42');
assert.strictEqual(util.format('%f', 1.5), '1.5');
assert.strictEqual(util.format('%f', -0.5), '-0.5');
assert.strictEqual(util.format('%f', Math.PI), '3.141592653589793');
assert.strictEqual(util.format('%f', ''), 'NaN');
assert.strictEqual(util.format('%f', Symbol('foo')), 'NaN');
assert.strictEqual(util.format('%f', 5n), '5');
assert.strictEqual(util.format('%f', Infinity), 'Infinity');
assert.strictEqual(util.format('%f', -Infinity), '-Infinity');
assert.strictEqual(util.format('%f %f', 42, 43), '42 43');
assert.strictEqual(util.format('%f %f', 42), '42 %f');

// String format specifier
assert.strictEqual(util.format('%s'), '%s');
assert.strictEqual(util.format('%s', undefined), 'undefined');
assert.strictEqual(util.format('%s', null), 'null');
assert.strictEqual(util.format('%s', 'foo'), 'foo');
assert.strictEqual(util.format('%s', 42), '42');
assert.strictEqual(util.format('%s', '42'), '42');
assert.strictEqual(util.format('%s', -0), '-0');
assert.strictEqual(util.format('%s', '-0.0'), '-0.0');
assert.strictEqual(util.format('%s %s', 42, 43), '42 43');
assert.strictEqual(util.format('%s %s', 42), '42 %s');
assert.strictEqual(util.format('%s', 42n), '42n');
assert.strictEqual(util.format('%s', Symbol('foo')), 'Symbol(foo)');
assert.strictEqual(util.format('%s', true), 'true');
assert.strictEqual(util.format('%s', { a: [1, 2, 3] }), '{ a: [Array] }');
assert.strictEqual(util.format('%s', { toString() { return 'Foo'; } }), 'Foo');
assert.strictEqual(util.format('%s', { toString: 5 }), '{ toString: 5 }');
assert.strictEqual(util.format('%s', () => 5), '() => 5');
assert.strictEqual(util.format('%s', Infinity), 'Infinity');
assert.strictEqual(util.format('%s', -Infinity), '-Infinity');

// String format specifier including `toString` properties on the prototype.
{
```

<a id="ref-q1-3"></a>
### [3] `test/parallel/test-util-format.js:54`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L54)

```javascript
assert.strictEqual(util.format('%d', 1.5), '1.5');
```

<a id="ref-q1-4"></a>
### [4] `test/parallel/test-util-format.js:57`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L57)

```javascript
assert.strictEqual(util.format('%d', ''), '0');
```

<a id="ref-q1-5"></a>
### [5] `test/parallel/test-util-format.js:56`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L56)

```javascript
assert.strictEqual(util.format('%d', -0.0), '-0');
```

<a id="ref-q1-6"></a>
### [6] `test/parallel/test-util-format.js:60-61`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L60-L61)

```javascript
assert.strictEqual(util.format('%d', Infinity), 'Infinity');
assert.strictEqual(util.format('%d', -Infinity), '-Infinity');
```

<a id="ref-q1-7"></a>
### [7] `test/parallel/test-util-format.js:59`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L59)

```javascript
assert.strictEqual(util.format('%d', Symbol()), 'NaN');
```

<a id="ref-q1-8"></a>
### [8] `test/parallel/test-util-format.js:69`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L69)

```javascript
  util.format('%d', 1180591620717411303424n),
```

<a id="ref-q1-9"></a>
### [9] `doc/api/util.md:467-469`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/doc/api/util.md#L467-L469)

```markdown
    pr-url: https://github.com/nodejs/node/pull/22097
    description: The `%d` and `%i` specifiers now support BigInt.
  - version: v8.4.0
```

<a id="ref-q1-10"></a>
### [10] `test/parallel/test-util-format.js:115`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L115)

```javascript
assert.strictEqual(util.format('%i', 1.5), '1');
```

<a id="ref-q1-11"></a>
### [11] `test/parallel/test-util-format.js:117`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L117)

```javascript
assert.strictEqual(util.format('%i', ''), 'NaN');
```

<a id="ref-q1-12"></a>
### [12] `test/parallel/test-util-format.js:116`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L116)

```javascript
assert.strictEqual(util.format('%i', -0.5), '-0');
```

<a id="ref-q1-13"></a>
### [13] `test/parallel/test-util-format.js:118-119`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L118-L119)

```javascript
assert.strictEqual(util.format('%i', Infinity), 'NaN');
assert.strictEqual(util.format('%i', -Infinity), 'NaN');
```

<a id="ref-q1-14"></a>
### [14] `test/parallel/test-util-format.js:120`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L120)

```javascript
assert.strictEqual(util.format('%i', Symbol()), 'NaN');
```

<a id="ref-q1-15"></a>
### [15] `test/parallel/test-util-format.js:128-129`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L128-L129)

```javascript
  util.format('%i', 1180591620717411303424n),
  '1180591620717411303424n'
```

<a id="ref-q1-16"></a>
### [16] `test/parallel/test-util-format.js:161`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L161)

```javascript
assert.strictEqual(util.format('%f', -0.5), '-0.5');
```

<a id="ref-q1-17"></a>
### [17] `test/parallel/test-util-format.js:163`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L163)

```javascript
assert.strictEqual(util.format('%f', ''), 'NaN');
```

<a id="ref-q1-18"></a>
### [18] `test/parallel/test-util-format.js:158`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L158)

```javascript
assert.strictEqual(util.format('%f', '-0.0'), '-0');
```

<a id="ref-q1-19"></a>
### [19] `test/parallel/test-util-format.js:166-167`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L166-L167)

```javascript
assert.strictEqual(util.format('%f', Infinity), 'Infinity');
assert.strictEqual(util.format('%f', -Infinity), '-Infinity');
```

<a id="ref-q1-20"></a>
### [20] `test/parallel/test-util-format.js:164`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L164)

```javascript
assert.strictEqual(util.format('%f', Symbol('foo')), 'NaN');
```

<a id="ref-q1-21"></a>
### [21] `test/parallel/test-util-format.js:165`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L165)

```javascript
assert.strictEqual(util.format('%f', 5n), '5');
```

<a id="ref-q1-22"></a>
### [22] `lib/util.js:1-100`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/lib/util.js#L1-L100)

<a id="ref-q1-23"></a>
### [23] `deps/v8/src/runtime/runtime-numbers.cc:21-45`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/src/runtime/runtime-numbers.cc#L21-L45)

```cpp
RUNTIME_FUNCTION(Runtime_StringParseInt) {
  HandleScope handle_scope(isolate);
  DCHECK_EQ(2, args.length());
  Handle<Object> string = args.at(0);
  Handle<Object> radix = args.at(1);

  // Convert {string} to a String first, and flatten it.
  Handle<String> subject;
  ASSIGN_RETURN_FAILURE_ON_EXCEPTION(isolate, subject,
                                     Object::ToString(isolate, string));
  subject = String::Flatten(isolate, subject);

  // Convert {radix} to Int32.
  if (!IsNumber(*radix)) {
    ASSIGN_RETURN_FAILURE_ON_EXCEPTION(isolate, radix,
                                       Object::ToNumber(isolate, radix));
  }
  int radix32 = DoubleToInt32(Object::NumberValue(*radix));
  if (radix32 != 0 && (radix32 < 2 || radix32 > 36)) {
    return ReadOnlyRoots(isolate).nan_value();
  }

  double result = StringToInt(isolate, subject, radix32);
  return *isolate->factory()->NewNumber(result);
}
```

<a id="ref-q1-24"></a>
### [24] `deps/v8/src/runtime/runtime-numbers.cc:48-57`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/src/runtime/runtime-numbers.cc#L48-L57)

```cpp
RUNTIME_FUNCTION(Runtime_StringParseFloat) {
  HandleScope shs(isolate);
  DCHECK_EQ(1, args.length());
  DirectHandle<String> subject = args.at<String>(0);

  double value = StringToDouble(isolate, subject, ALLOW_TRAILING_JUNK,
                                std::numeric_limits<double>::quiet_NaN());

  return *isolate->factory()->NewNumber(value);
}
```

<a id="ref-q1-25"></a>
### [25] `deps/v8/src/numbers/conversions.cc:991-994`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/src/numbers/conversions.cc#L991-L994)

```cpp
double StringToInt(Isolate* isolate, DirectHandle<String> string, int radix) {
  NumberParseIntHelper helper(string, radix);
  return helper.GetResult();
}
```

<a id="ref-q1-26"></a>
### [26] `deps/v8/src/numbers/conversions.cc:1131-1195`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/deps/v8/src/numbers/conversions.cc#L1131-L1195)

```cpp
std::string_view DoubleToStringView(double v, base::Vector<char> buffer) {
  switch (FPCLASSIFY_NAMESPACE::fpclassify(v)) {
    case FP_NAN:
      return "NaN";
    case FP_INFINITE:
      return (v < 0.0 ? "-Infinity" : "Infinity");
    case FP_ZERO:
      return "0";
    default: {
      if (IsInt32Double(v)) {
        // This will trigger if v is -0 and -0.0 is stringified to "0".
        // (see ES section 7.1.12.1 #sec-tostring-applied-to-the-number-type)
        return IntToStringView(FastD2I(v), buffer);
      }
      SimpleStringBuilder builder(buffer.begin(), buffer.size());
      auto d = jkj::dragonbox::to_decimal(v);

      if (d.is_negative) builder.AddCharacter('-');

      // Only in debug-builds the buffer is null-terminated.
      constexpr int kDecimalRepLength =
          base::kBase10MaximalLength + (DEBUG_BOOL ? 1 : 0);
      char decimal_rep[kDecimalRepLength];
      int length = SignificandToChars(d.significand, decimal_rep);
#ifdef DEBUG
      // Null-terminate decimal rep for DCHECKs in SimpleStringBuilder.
      DCHECK_LT(length, kDecimalRepLength);
      decimal_rep[length] = '\0';
#endif
      int decimal_point = length + d.exponent;

      if (length <= decimal_point && decimal_point <= 21) {
        // ECMA-262 section 9.8.1 step 6.
        builder.AddString(decimal_rep, length);
        builder.AddPadding('0', decimal_point - length);

      } else if (0 < decimal_point && decimal_point <= 21) {
        // ECMA-262 section 9.8.1 step 7.
        builder.AddSubstring(decimal_rep, decimal_point);
        builder.AddCharacter('.');
        builder.AddString(decimal_rep + decimal_point, length - decimal_point);

      } else if (decimal_point <= 0 && decimal_point > -6) {
        // ECMA-262 section 9.8.1 step 8.
        builder.AddStringLiteral("0.");
        builder.AddPadding('0', -decimal_point);
        builder.AddString(decimal_rep, length);

      } else {
        // ECMA-262 section 9.8.1 step 9 and 10 combined.
        builder.AddCharacter(decimal_rep[0]);
        if (length != 1) {
          builder.AddCharacter('.');
          builder.AddString(decimal_rep + 1, length - 1);
        }
        builder.AddCharacter('e');
        builder.AddCharacter((decimal_point >= 0) ? '+' : '-');
        int exponent = decimal_point - 1;
        if (exponent < 0) exponent = -exponent;
        builder.AddExponent(exponent);
      }
      return {buffer.begin(), builder.Finalize()};
    }
  }
}
```

<a id="ref-q1-27"></a>
### [27] `test/parallel/test-util-format.js:69-70`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L69-L70)

```javascript
  util.format('%d', 1180591620717411303424n),
  '1180591620717411303424n'
```

<a id="ref-q1-28"></a>
### [28] `test/parallel/test-util-format.js:77-108`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-format.js#L77-L108)

```javascript
{
  const { numericSeparator } = util.inspect.defaultOptions;
  util.inspect.defaultOptions.numericSeparator = true;

  assert.strictEqual(
    util.format('%d', 1180591620717411303424),
    '1.1805916207174113e+21'
  );

  assert.strictEqual(
    util.format(
      // eslint-disable-next-line no-loss-of-precision
      '%d %s %i', 118059162071741130342, 118059162071741130342, 123_123_123),
    '118_059_162_071_741_140_000 118_059_162_071_741_140_000 123_123_123'
  );

  assert.strictEqual(
    util.format(
      '%d %s',
      1_180_591_620_717_411_303_424n,
      12_345_678_901_234_567_890_123n
    ),
    '1_180_591_620_717_411_303_424n 12_345_678_901_234_567_890_123n'
  );

  assert.strictEqual(
    util.format('%i', 1_180_591_620_717_411_303_424n),
    '1_180_591_620_717_411_303_424n'
  );

  util.inspect.defaultOptions.numericSeparator = numericSeparator;
}
```

<a id="ref-q1-29"></a>
### [29] `test/parallel/test-util-inspect.js:3773-3847`
Source: [nodejs/node @ 159ae48f](https://github.com/nodejs/node/blob/159ae48f/test/parallel/test-util-inspect.js#L3773-L3847)

```javascript
{
  const { numericSeparator } = util.inspect.defaultOptions;
  util.inspect.defaultOptions.numericSeparator = true;

  assert.strictEqual(
    // eslint-disable-next-line no-loss-of-precision
    util.inspect(1234567891234567891234),
    '1.234567891234568e+21'
  );
  assert.strictEqual(
    util.inspect(123456789.12345678),
    '123_456_789.123_456_78'
  );

  assert.strictEqual(util.inspect(10_000_000), '10_000_000');
  assert.strictEqual(util.inspect(1_000_000), '1_000_000');
  assert.strictEqual(util.inspect(100_000), '100_000');
  assert.strictEqual(util.inspect(99_999.9), '99_999.9');
  assert.strictEqual(util.inspect(9_999), '9_999');
  assert.strictEqual(util.inspect(999), '999');
  assert.strictEqual(util.inspect(NaN), 'NaN');
  assert.strictEqual(util.inspect(Infinity), 'Infinity');
  assert.strictEqual(util.inspect(-Infinity), '-Infinity');

  assert.strictEqual(
    util.inspect(new Float64Array([100_000_000])),
    'Float64Array(1) [ 100_000_000 ]'
  );
  assert.strictEqual(
    util.inspect(new BigInt64Array([9_100_000_100n])),
    'BigInt64Array(1) [ 9_100_000_100n ]'
  );

  assert.strictEqual(
    util.inspect(123456789),
    '123_456_789'
  );
  assert.strictEqual(
    util.inspect(123456789n),
    '123_456_789n'
  );

  util.inspect.defaultOptions.numericSeparator = numericSeparator;

  assert.strictEqual(
    util.inspect(123456789.12345678, { numericSeparator: true }),
    '123_456_789.123_456_78'
  );

  assert.strictEqual(
    util.inspect(-123456789.12345678, { numericSeparator: true }),
    '-123_456_789.123_456_78'
  );

  // Regression test for https://github.com/nodejs/node/issues/59376
  // numericSeparator should work correctly for negative fractional numbers
  {
    // Test the exact values from the GitHub issue
    const values = [0.1234, -0.12, -0.123, -0.1234, -1.234];
    assert.strictEqual(
      util.inspect(values, { numericSeparator: true }),
      '[ 0.123_4, -0.12, -0.123, -0.123_4, -1.234 ]'
    );

    // Test individual negative fractional numbers between -1 and 0
    assert.strictEqual(
      util.inspect(-0.1234, { numericSeparator: true }),
      '-0.123_4'
    );
    assert.strictEqual(
      util.inspect(-0.12345, { numericSeparator: true }),
      '-0.123_45'
    );
  }
}
```

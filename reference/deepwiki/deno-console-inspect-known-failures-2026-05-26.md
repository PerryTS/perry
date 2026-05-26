# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests document console.dir / util.inspect behavior relevant to known-failure cleanup: honoring depth options, showing hidden properties when showHidden is true, formatting function values without invoking user toString, and rendering circular object references? Focus only on public source/tests useful as reference material for revalidating PerryTS/perry node:console known-failure entries.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_ff8f96e1-c0c6-4449-a4e5-a4c6c2935f39

**Question**: For Node compatibility in Deno, what source files or tests document console.dir / util.inspect behavior relevant to known-failure cleanup: honoring depth options, showing hidden properties when showHidden is true, formatting function values without invoking user toString, and rendering circular object references? Focus only on public source/tests useful as reference material for revalidating PerryTS/perry node:console known-failure entries.

The `console.dir` and `util.inspect` behavior in Deno, particularly concerning depth options, hidden properties, function formatting, and circular references, is primarily documented in `ext/node/polyfills/internal/util/inspect.mjs` [1](#ref-q1-1)  and tested in `tests/unit/console_test.ts` [2](#ref-q1-2)  and `tests/unit_node/util_test.ts` [3](#ref-q1-3) . The `cli/tsc/dts/node/util.d.cts` file also provides the TypeScript definitions for `util.inspect` options [4](#ref-q1-4) .

## `util.inspect` Implementation Details

The core logic for `util.inspect` is found in `ext/node/polyfills/internal/util/inspect.mjs` [5](#ref-q1-5) .

### Depth Options
The `depth` option is handled within the `inspect` function [6](#ref-q1-6) . The `inspectDefaultOptions` object defines the default `depth` as `2` [7](#ref-q1-7) . The `formatValue` function, which is called by `inspect`, uses this `depth` to control recursion [8](#ref-q1-8) . Tests for `depth` are present in `tests/unit/console_test.ts`, demonstrating how objects are truncated at specified depths [9](#ref-q1-9) .

### Showing Hidden Properties
The `showHidden` option is also part of `inspectDefaultOptions` [10](#ref-q1-10)  and can be set via the `opts` argument to `inspect` [11](#ref-q1-11) . When `showHidden` is true, non-enumerable properties are included in the formatted output [12](#ref-q1-12) . The `formatRaw` function in `ext/web/01_console.js` uses `ctx.showHidden` to determine which keys to retrieve [13](#ref-q1-13) .

### Formatting Function Values
The `inspectAnonymousFunctions` test in `tests/unit/console_test.ts` shows how anonymous functions are formatted as `[Function (anonymous)]`, `[AsyncFunction (anonymous)]`, etc., without invoking a `toString` method on the user's function [14](#ref-q1-14) .

### Rendering Circular Object References
The `cli/tsc/dts/node/util.d.cts` file explicitly mentions that `util.inspect` handles circular references by using reference indices like `<ref *1>` [15](#ref-q1-15) . The `formatValue` function in `ext/web/01_console.js` manages custom inspection via `Symbol.for("nodejs.util.inspect.custom")` [16](#ref-q1-16) , which is crucial for handling complex objects and preventing infinite recursion in circular structures. The `inspect` function in `ext/node/polyfills/internal/util/inspect.mjs` initializes a `seen` array in its context (`ctx`) to track visited objects and detect circular references [17](#ref-q1-17) .

## `console.dir` Behavior
The `console.dir` function in Deno is implemented to respect group indentation [18](#ref-q1-18) . The `console.dir` tests in `tests/unit/console_test.ts` confirm its basic functionality and how it handles `indentLevel` options [19](#ref-q1-19) . The `ext/node/polyfills/internal/console/constructor.mjs` file imports the `inspect` and `formatWithOptions` functions from `ext:deno_node/internal/util/inspect.mjs` [20](#ref-q1-20) , indicating that `console.dir` likely leverages the same underlying inspection logic as `util.inspect`.

## Notes
The `ext/node/polyfills/internal/buffer.mjs` file also contains a `Buffer.prototype[customInspectSymbol]` implementation [21](#ref-q1-21) , which is relevant for how `Buffer` objects are displayed during inspection. This custom inspect function formats the buffer content as hexadecimal and truncates it if it exceeds `INSPECT_MAX_BYTES_` [22](#ref-q1-22) .

Wiki pages you might want to explore:
- [Storage, Cache, and Web APIs (denoland/deno)](/wiki/denoland/deno#6.4)
- [Buffer and Core APIs (denoland/deno)](/wiki/denoland/deno#7.2)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_ff8f96e1-c0c6-4449-a4e5-a4c6c2935f39

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/internal/util/inspect.mjs:121-300`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L121-L300)

```
  bgMagentaBright: [105, defaultBG],
  bgCyanBright: [106, defaultBG],
  bgWhiteBright: [107, defaultBG],
};

function defineColorAlias(target, alias) {
  ObjectDefineProperty(inspect.colors, alias, {
    __proto__: null,
    get() {
      return this[target];
    },
    set(value) {
      this[target] = value;
    },
    configurable: true,
    enumerable: false,
  });
}

defineColorAlias("gray", "grey");
defineColorAlias("gray", "blackBright");
defineColorAlias("bgGray", "bgGrey");
defineColorAlias("bgGray", "bgBlackBright");
defineColorAlias("dim", "faint");
defineColorAlias("strikethrough", "crossedout");
defineColorAlias("strikethrough", "strikeThrough");
defineColorAlias("strikethrough", "crossedOut");
defineColorAlias("hidden", "conceal");
defineColorAlias("inverse", "swapColors");
defineColorAlias("inverse", "swapcolors");
defineColorAlias("doubleunderline", "doubleUnderline");

// TODO(BridgeAR): Add function style support for more complex styles.
// Don't use 'blue' not visible on cmd.exe
inspect.styles = ObjectAssign(ObjectCreate(null), {
  special: "cyan",
  number: "yellow",
  bigint: "yellow",
  boolean: "yellow",
  undefined: "grey",
  null: "bold",
  string: "green",
  symbol: "green",
  date: "magenta",
  // "name": intentionally not styling
  // TODO(BridgeAR): Highlight regular expressions properly.
  regexp: "red",
  module: "underline",
});

const inspectDefaultOptions = {
  indentationLvl: 0,
  currentDepth: 0,
  stylize: stylizeNoColor,

  showHidden: false,
  depth: 2,
  colors: false,
  showProxy: false,
  breakLength: 80,
  escapeSequences: true,
  compact: 3,
  sorted: false,
  getters: false,

  // node only
  maxArrayLength: 100,
  maxStringLength: 10000, // deno: strAbbreviateSize: 100
  customInspect: true,

  // deno only
  /** You can override the quotes preference in inspectString.
   * Used by util.inspect() */
  // TODO(kt3k): Consider using symbol as a key to hide this from the public
  // API.
  quotes: ["'", '"', "`"],
  iterableLimit: Infinity, // similar to node's maxArrayLength, but doesn't only apply to arrays
  trailingComma: false,

  inspect,

  // TODO(@crowlKats): merge into indentationLvl
  indentLevel: 0,
};

/**
 * Echos the value of any input. Tries to print the value out
 * in the best way possible given the different types.
 */
/* Legacy: value, showHidden, depth, colors */
function inspect(value, opts) {
  // Default options
  const ctx = {
    budget: {},
    seen: [],
    ...inspectDefaultOptions,
  };
  if (arguments.length > 1) {
    // Legacy...
    if (arguments.length > 2) {
      if (arguments[2] !== undefined) {
        ctx.depth = arguments[2];
      }
      if (arguments.length > 3 && arguments[3] !== undefined) {
        ctx.colors = arguments[3];
      }
    }
    // Set user-specified options
    if (typeof opts === "boolean") {
      ctx.showHidden = opts;
    } else if (opts) {
      const optKeys = ObjectKeys(opts);
      for (let i = 0; i < optKeys.length; ++i) {
        const key = optKeys[i];
        // TODO(BridgeAR): Find a solution what to do about stylize. Either make
        // this function public or add a new API with a similar or better
        // functionality.
        if (
          ObjectPrototypeHasOwnProperty(inspectDefaultOptions, key) ||
          key === "stylize"
        ) {
          ctx[key] = opts[key];
        } else if (ctx.userOptions === undefined) {
          // This is required to pass through the actual user input.
          ctx.userOptions = opts;
        }
      }
    }
  }
  if (ctx.colors) {
    ctx.stylize = createStylizeWithColor(inspect.styles, inspect.colors);
  }
  if (ctx.maxArrayLength === null) ctx.maxArrayLength = Infinity;
  if (ctx.maxStringLength === null) ctx.maxStringLength = Infinity;
  return formatValue(ctx, value, 0);
}
const customInspectSymbol = SymbolFor("nodejs.util.inspect.custom");
inspect.custom = customInspectSymbol;

ObjectDefineProperty(inspect, "defaultOptions", {
  __proto__: null,
  get() {
    return inspectDefaultOptions;
  },
  set(options) {
    validateObject(options, "options");
    return ObjectAssign(inspectDefaultOptions, options);
  },
});

function stylizeNoColor(str) {
  return str;
}

const builtInObjects = new SafeSet(
  ArrayPrototypeFilter(
    ObjectGetOwnPropertyNames(globalThis),
    (e) => RegExpPrototypeTest(new SafeRegExp(/^[A-Z][a-zA-Z0-9]+$/), e),
  ),
);

// Regex used for ansi escape code splitting
// Adopted from https://github.com/chalk/ansi-regex/blob/HEAD/index.js
// License: MIT, authors: @sindresorhus, Qix-, arjunmehta and LitoMore
// Matches all ansi escape code sequences in a string
const ansiPattern = "[\\u001B\\u009B][[\\]()#;?]*" +
  "(?:(?:(?:(?:;[-a-zA-Z\\d\\/#&.:=?%@~_]+)*" +
  "|[a-zA-Z\\d]+(?:;[-a-zA-Z\\d\\/#&.:=?%@~_]*)*)" +
  "?(?:\\u0007|\\u001B\\u005C|\\u009C))" +
  "|(?:(?:\\d{1,4}(?:;\\d{0,4})*)?[\\dA-PR-TZcf-nq-uy=><~]))";
const ansi = new SafeRegExp(ansiPattern, "g");

const reEmojiPresentation = new SafeRegExp("^\\p{Emoji_Presentation}$", "u");

/**
 * Returns the number of columns required to display the given string.
 */
function getStringWidth(str, removeControlChars = true) {
  let width = 0;
```

<a id="ref-q1-2"></a>
### [2] `tests/unit/console_test.ts:549-614`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L549-L614)

```typescript
});

Deno.test(function consoleTestStringifyWithDepth() {
  // deno-lint-ignore no-explicit-any
  const nestedObj: any = { a: { b: { c: { d: { e: { f: 42 } } } } } };
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 3 })),
    "{\n  a: { b: { c: { d: [Object] } } }\n}",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 4 })),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 0 })),
    "{ a: [Object] }",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj])),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
  // test inspect is working the same way
  assertEquals(
    stripAnsiCode(Deno.inspect(nestedObj, { depth: 4 })),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
});

Deno.test(function consoleTestStringifyLargeObject() {
  const obj = {
    a: 2,
    o: {
      a: "1",
      b: "2",
      c: "3",
      d: "4",
      e: "5",
      f: "6",
      g: 10,
      asd: 2,
      asda: 3,
      x: { a: "asd", x: 3 },
    },
  };
  assertEquals(
    stringify(obj),
    `{
  a: 2,
  o: {
    a: "1",
    b: "2",
    c: "3",
    d: "4",
    e: "5",
    f: "6",
    g: 10,
    asd: 2,
    asda: 3,
    x: { a: "asd", x: 3 }
  }
}`,
  );
});

Deno.test(function consoleTestStringifyIterable() {
  const shortArray = [1, 2, 3, 4, 5];
```

<a id="ref-q1-3"></a>
### [3] `tests/unit_node/util_test.ts:1-46`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/util_test.ts#L1-L46)

```typescript
// Copyright 2018-2026 the Deno authors. MIT license.

import {
  assert,
  assertEquals,
  assertStrictEquals,
  assertThrows,
} from "@std/assert";
import { stripAnsiCode } from "@std/fmt/colors";
import * as util from "node:util";
import utilDefault from "node:util";
import { Buffer } from "node:buffer";

Deno.test({
  name: "[util] format",
  fn() {
    assertEquals(util.format("%o", [10, 11]), "[ 10, 11, [length]: 2 ]");
  },
});

Deno.test({
  name: "[util] inspect.custom",
  fn() {
    assertEquals(util.inspect.custom, Symbol.for("nodejs.util.inspect.custom"));
  },
});

Deno.test({
  name: "[util] inspect",
  fn() {
    assertEquals(stripAnsiCode(util.inspect({ foo: 123 })), "{ foo: 123 }");
    assertEquals(stripAnsiCode(util.inspect("foo")), "'foo'");
    assertEquals(
      stripAnsiCode(util.inspect("Deno's logo is so cute.")),
      `"Deno's logo is so cute."`,
    );
    assertEquals(
      stripAnsiCode(util.inspect([1, 2, 3, 4, 5, 6, 7])),
      `[
  1, 2, 3, 4,
  5, 6, 7
]`,
    );
  },
});
```

<a id="ref-q1-4"></a>
### [4] `cli/tsc/dts/node/util.d.cts:13-60`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L13-L60)

```
    export interface InspectOptions {
        /**
         * If `true`, object's non-enumerable symbols and properties are included in the formatted result.
         * `WeakMap` and `WeakSet` entries are also included as well as user defined prototype properties (excluding method properties).
         * @default false
         */
        showHidden?: boolean | undefined;
        /**
         * Specifies the number of times to recurse while formatting object.
         * This is useful for inspecting large objects.
         * To recurse up to the maximum call stack size pass `Infinity` or `null`.
         * @default 2
         */
        depth?: number | null | undefined;
        /**
         * If `true`, the output is styled with ANSI color codes. Colors are customizable.
         */
        colors?: boolean | undefined;
        /**
         * If `false`, `[util.inspect.custom](depth, opts, inspect)` functions are not invoked.
         * @default true
         */
        customInspect?: boolean | undefined;
        /**
         * If `true`, `Proxy` inspection includes the target and handler objects.
         * @default false
         */
        showProxy?: boolean | undefined;
        /**
         * Specifies the maximum number of `Array`, `TypedArray`, `WeakMap`, and `WeakSet` elements
         * to include when formatting. Set to `null` or `Infinity` to show all elements.
         * Set to `0` or negative to show no elements.
         * @default 100
         */
        maxArrayLength?: number | null | undefined;
        /**
         * Specifies the maximum number of characters to
         * include when formatting. Set to `null` or `Infinity` to show all elements.
         * Set to `0` or negative to show no characters.
         * @default 10000
         */
        maxStringLength?: number | null | undefined;
        /**
         * The length at which input values are split across multiple lines.
         * Set to `Infinity` to format the input as a single line
         * (in combination with `compact` set to `true` or any number >= `1`).
         * @default 80
         */
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/internal/util/inspect.mjs:211-255`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L211-L255)

```
function inspect(value, opts) {
  // Default options
  const ctx = {
    budget: {},
    seen: [],
    ...inspectDefaultOptions,
  };
  if (arguments.length > 1) {
    // Legacy...
    if (arguments.length > 2) {
      if (arguments[2] !== undefined) {
        ctx.depth = arguments[2];
      }
      if (arguments.length > 3 && arguments[3] !== undefined) {
        ctx.colors = arguments[3];
      }
    }
    // Set user-specified options
    if (typeof opts === "boolean") {
      ctx.showHidden = opts;
    } else if (opts) {
      const optKeys = ObjectKeys(opts);
      for (let i = 0; i < optKeys.length; ++i) {
        const key = optKeys[i];
        // TODO(BridgeAR): Find a solution what to do about stylize. Either make
        // this function public or add a new API with a similar or better
        // functionality.
        if (
          ObjectPrototypeHasOwnProperty(inspectDefaultOptions, key) ||
          key === "stylize"
        ) {
          ctx[key] = opts[key];
        } else if (ctx.userOptions === undefined) {
          // This is required to pass through the actual user input.
          ctx.userOptions = opts;
        }
      }
    }
  }
  if (ctx.colors) {
    ctx.stylize = createStylizeWithColor(inspect.styles, inspect.colors);
  }
  if (ctx.maxArrayLength === null) ctx.maxArrayLength = Infinity;
  if (ctx.maxStringLength === null) ctx.maxStringLength = Infinity;
  return formatValue(ctx, value, 0);
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/internal/util/inspect.mjs:221-223`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L221-L223)

```
      if (arguments[2] !== undefined) {
        ctx.depth = arguments[2];
      }
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/internal/util/inspect.mjs:177`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L177)

```
  depth: 2,
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/internal/util/inspect.mjs:255`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L255)

```
  return formatValue(ctx, value, 0);
```

<a id="ref-q1-9"></a>
### [9] `tests/unit/console_test.ts:551-574`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L551-L574)

```typescript
Deno.test(function consoleTestStringifyWithDepth() {
  // deno-lint-ignore no-explicit-any
  const nestedObj: any = { a: { b: { c: { d: { e: { f: 42 } } } } } };
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 3 })),
    "{\n  a: { b: { c: { d: [Object] } } }\n}",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 4 })),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj], { depth: 0 })),
    "{ a: [Object] }",
  );
  assertEquals(
    stripAnsiCode(inspectArgs([nestedObj])),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
  // test inspect is working the same way
  assertEquals(
    stripAnsiCode(Deno.inspect(nestedObj, { depth: 4 })),
    "{\n  a: {\n    b: { c: { d: { e: [Object] } } }\n  }\n}",
  );
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/internal/util/inspect.mjs:176`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L176)

```
  showHidden: false,
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/internal/util/inspect.mjs:229-230`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L229-L230)

```
    if (typeof opts === "boolean") {
      ctx.showHidden = opts;
```

<a id="ref-q1-12"></a>
### [12] `cli/tsc/dts/node/util.d.cts:15-18`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L15-L18)

```
         * If `true`, object's non-enumerable symbols and properties are included in the formatted result.
         * `WeakMap` and `WeakSet` entries are also included as well as user defined prototype properties (excluding method properties).
         * @default false
         */
```

<a id="ref-q1-13"></a>
### [13] `ext/web/01_console.js:722-723`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/01_console.js#L722-L723)

```javascript
  let protoProps;
  if (ctx.showHidden && (recurseTimes <= ctx.depth || ctx.depth === null)) {
```

<a id="ref-q1-14"></a>
### [14] `tests/unit/console_test.ts:2637-2653`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L2637-L2653)

```typescript
Deno.test(function inspectAnonymousFunctions() {
  assertEquals(Deno.inspect(() => {}), "[Function (anonymous)]");
  assertEquals(Deno.inspect(function () {}), "[Function (anonymous)]");
  assertEquals(Deno.inspect(async () => {}), "[AsyncFunction (anonymous)]");
  assertEquals(
    Deno.inspect(async function () {}),
    "[AsyncFunction (anonymous)]",
  );
  assertEquals(
    Deno.inspect(function* () {}),
    "[GeneratorFunction (anonymous)]",
  );
  assertEquals(
    Deno.inspect(async function* () {}),
    "[AsyncGeneratorFunction (anonymous)]",
  );
});
```

<a id="ref-q1-15"></a>
### [15] `cli/tsc/dts/node/util.d.cts:454-469`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/util.d.cts#L454-L469)

```
     *
     * ```js
     * import { inspect } from 'node:util';
     *
     * const obj = {};
     * obj.a = [obj];
     * obj.b = {};
     * obj.b.inner = obj.b;
     * obj.b.obj = obj;
     *
     * console.log(inspect(obj));
     * // <ref *1> {
     * //   a: [ [Circular *1] ],
     * //   b: <ref *2> { inner: [Circular *2], obj: [Circular *1] }
     * // }
     * ```
```

<a id="ref-q1-16"></a>
### [16] `ext/web/01_console.js:563-567`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/web/01_console.js#L563-L567)

```javascript
  if (ctx.customInspect) {
    const inspectTarget = proxyDetails ? proxyDetails[0] : value;
    if (
      ReflectHas(inspectTarget, customInspect) &&
      typeof inspectTarget[customInspect] === "function"
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/internal/util/inspect.mjs:215`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/util/inspect.mjs#L215)

```
    seen: [],
```

<a id="ref-q1-18"></a>
### [18] `tests/unit/console_test.ts:1568-1578`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L1568-L1578)

```typescript
Deno.test(function consoleGroupDir() {
  mockConsole((console, out) => {
    console.dir("1");
    console.group();
    console.dir("2");
    console.group();
    console.dir("3");
    console.groupEnd();
    console.dir("4");
    console.groupEnd();
    console.dir("5");
```

<a id="ref-q1-19"></a>
### [19] `tests/unit/console_test.ts:2107-2115`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit/console_test.ts#L2107-L2115)

```typescript
Deno.test(function consoleDir() {
  mockConsole((console, out) => {
    console.dir("DIR");
    assertEquals(out.toString(), "DIR\n");
  });
  mockConsole((console, out) => {
    console.dir("DIR", { indentLevel: 2 });
    assertEquals(out.toString(), "    DIR\n");
  });
```

<a id="ref-q1-20"></a>
### [20] `ext/node/polyfills/internal/console/constructor.mjs:73-75`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/console/constructor.mjs#L73-L75)

```
  formatWithOptions,
  inspect,
} = core.loadExtScript("ext:deno_node/internal/util/inspect.mjs");
```

<a id="ref-q1-21"></a>
### [21] `ext/node/polyfills/internal/buffer.mjs:799-800`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L799-L800)

```
Buffer.prototype[customInspectSymbol] =
  Buffer.prototype.inspect =
```

<a id="ref-q1-22"></a>
### [22] `ext/node/polyfills/internal/buffer.mjs:801-818`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/internal/buffer.mjs#L801-L818)

```
    function inspect(_, ctx) {
      let str = "";
      str = StringPrototypeTrim(
        StringPrototypeReplace(
          // Use Buffer.prototype.toString so the inspect output stays
          // hex-formatted even when the receiver is a Uint8Array.
          FunctionPrototypeCall(
            Buffer.prototype.toString,
            this,
            "hex",
            0,
            INSPECT_MAX_BYTES_,
          ),
          SPACER_PATTERN,
          "$1 ",
        ),
      );
      if (this.length > INSPECT_MAX_BYTES_) {
```

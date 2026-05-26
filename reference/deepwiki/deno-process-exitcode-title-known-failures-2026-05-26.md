# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what behavior or tests establish Node-compatible process.exitCode and process.title semantics: process.exitCode starts as undefined, assigned numeric values round-trip through reads with Node coercion, and process.title can be assigned and read back? Focus only on source files or tests useful as public reference material for revalidating stale PerryTS/perry node:process known-failure entries.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_dc745f2f-be2c-447f-bfaa-6e8591d037f1

**Question**: For Node compatibility in Deno, what behavior or tests establish Node-compatible process.exitCode and process.title semantics: process.exitCode starts as undefined, assigned numeric values round-trip through reads with Node coercion, and process.title can be assigned and read back? Focus only on source files or tests useful as public reference material for revalidating stale PerryTS/perry node:process known-failure entries.

# Answer

You're asking about the Node.js compatibility implementation in Deno for `process.exitCode` and `process.title` semantics, specifically the behaviors and tests that validate: (1) `process.exitCode` starts as `undefined`, (2) numeric values assigned to it round-trip correctly with Node coercion, and (3) `process.title` can be assigned and read back.

## process.exitCode Implementation and Tests

The `process.exitCode` property is defined in [1](#ref-q1-1) . [2](#ref-q1-2) The internal state variable `ProcessExitCode` is initialized as `undefined`, matching Node.js behavior.</cite> The getter returns this value directly, while the setter implements Node-compatible coercion: [3](#ref-q1-3) it accepts `number | string | null | undefined`, validates that numeric strings are valid integers (including hex notation like `'0x10'`), and stores the original value while calling `denoOs.setExitCode()` with the parsed numeric result.</cite>

The unit tests in [4](#ref-q1-4)  validate this behavior comprehensively. The test at lines 882–893 confirms that `process.exitCode` starts as `undefined`, can be set to numeric values (127), and rejects invalid strings like `"asdf"` while accepting valid numeric strings like `"10"`. The integration tests at lines 907–920 verify that exit codes set via `process.exitCode` actually affect the process exit code, including hex notation (`'0x10'` → exit code 16).</cite>

## process.title Implementation and Tests

The `process.title` property is defined in [5](#ref-q1-5) . It uses a getter/setter pattern where the getter returns a stored `processTitle` variable (or the `execPath` if unset), and the setter coerces the value to a string and calls `op_node_process_set_title()`.</cite>

The unit test in [6](#ref-q1-6)  validates the round-trip behavior: it confirms the default value equals `process.execPath`, sets `process.title` to `"foo"`, verifies the read-back value matches, and restores the original.</cite>

## Public Reference Material

For validating stale PerryTS/perry node:process known-failure entries, the most useful public reference files are:

1. **Source implementation**: [1](#ref-q1-1)  for `exitCode` and [5](#ref-q1-5)  for `title`
2. **Unit tests**: [4](#ref-q1-4)  for comprehensive `exitCode` validation including coercion and hex notation
3. **Integration tests**: The same test file demonstrates actual process exit code behavior

These files establish the complete semantics: `exitCode` undefined initialization, numeric/string coercion with validation, and `title` read/write round-tripping.

## Notes

The implementation stores the original assigned value in `ProcessExitCode` (line 1049) while passing the parsed numeric result to the OS layer via `denoOs.setExitCode()`. This dual-storage approach ensures that reading `process.exitCode` returns the originally-assigned value (preserving type information for Node compatibility) while the actual exit code used by the process is the parsed numeric value. The test at line 891 (`process.exitCode = "10"`) demonstrates this: the string `"10"` is accepted and stored, but the process exits with code 10.

Wiki pages you might want to explore:
- [Node.js Compatibility Layer (denoland/deno)](/wiki/denoland/deno#7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_dc745f2f-be2c-447f-bfaa-6e8591d037f1

## References

<a id="ref-q1-1"></a>
### [1] `ext/node/polyfills/process.ts:1022-1051`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1022-L1051)

```typescript
/** https://nodejs.org/api/process.html#processexitcode_1 */
Object.defineProperty(process, "exitCode", {
  get() {
    return ProcessExitCode;
  },
  set(code: number | string | null | undefined) {
    let parsedCode: number;
    if (code == null) {
      parsedCode = 0;
    } else if (typeof code === "number") {
      if (!Number.isInteger(code)) {
        throw new ERR_OUT_OF_RANGE("code", "an integer", code);
      }
      parsedCode = code;
    } else if (typeof code === "string") {
      if (
        code === "" || !Number.isFinite(Number(code)) ||
        !Number.isInteger(Number(code))
      ) {
        throw new ERR_INVALID_ARG_TYPE("code", "integer", code);
      }
      parsedCode = Number(code);
    } else {
      throw new ERR_INVALID_ARG_TYPE("code", "integer", code);
    }

    denoOs.setExitCode(parsedCode);
    ProcessExitCode = code;
  },
});
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/process.ts:143-145`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L143-L145)

```typescript
// In Node, `process.exitCode` is initially `undefined` until set.
// And retains any value as long as it's nullish or number-ish.
let ProcessExitCode: undefined | null | string | number;
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/process.ts:1027-1050`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L1027-L1050)

```typescript
  set(code: number | string | null | undefined) {
    let parsedCode: number;
    if (code == null) {
      parsedCode = 0;
    } else if (typeof code === "number") {
      if (!Number.isInteger(code)) {
        throw new ERR_OUT_OF_RANGE("code", "an integer", code);
      }
      parsedCode = code;
    } else if (typeof code === "string") {
      if (
        code === "" || !Number.isFinite(Number(code)) ||
        !Number.isInteger(Number(code))
      ) {
        throw new ERR_INVALID_ARG_TYPE("code", "integer", code);
      }
      parsedCode = Number(code);
    } else {
      throw new ERR_INVALID_ARG_TYPE("code", "integer", code);
    }

    denoOs.setExitCode(parsedCode);
    ProcessExitCode = code;
  },
```

<a id="ref-q1-4"></a>
### [4] `tests/unit_node/process_test.ts:882-920`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L882-L920)

```typescript
Deno.test("process.exitCode", () => {
  assertEquals(process.exitCode, undefined);
  process.exitCode = 127;
  assertEquals(process.exitCode, 127);
  assertThrows(() => {
    // deno-lint-ignore no-explicit-any
    (process.exitCode as any) = "asdf";
  });
  // deno-lint-ignore no-explicit-any
  (process.exitCode as any) = "10";
  process.exitCode = undefined; // reset
});

async function exitCodeTest(codeText: string, expectedExitCode: number) {
  const command = new Deno.Command(Deno.execPath(), {
    args: [
      "eval",
      codeText,
    ],
    cwd: testDir,
  });
  const { code } = await command.output();
  assertEquals(code, expectedExitCode);
}

Deno.test("process.exitCode in should change exit code", async () => {
  await exitCodeTest(
    "import process from 'node:process'; process.exitCode = 127;",
    127,
  );
  await exitCodeTest(
    "import process from 'node:process'; process.exitCode = '10';",
    10,
  );
  await exitCodeTest(
    "import process from 'node:process'; process.exitCode = '0x10';",
    16,
  );
});
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/process.ts:843-855`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/process.ts#L843-L855)

```typescript
let processTitle: string | undefined;
Object.defineProperty(process, "title", {
  get() {
    if (processTitle == null) {
      return String(execPath);
    }
    return processTitle;
  },
  set(value) {
    processTitle = `${value}`;
    op_node_process_set_title(processTitle);
  },
});
```

<a id="ref-q1-6"></a>
### [6] `tests/unit_node/process_test.ts:1057-1069`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/unit_node/process_test.ts#L1057-L1069)

```typescript
Deno.test({
  name: "process.title",
  fn() {
    // Default process.title should be the execPath (matches Node.js behavior)
    assertEquals(process.title, process.execPath);
    // Setting process.title should work
    const original = process.title;
    process.title = "foo";
    assertEquals(process.title, "foo");
    // Restore
    process.title = original;
  },
});
```

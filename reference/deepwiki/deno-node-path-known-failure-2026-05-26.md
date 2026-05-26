# DeepWiki Response

Repository: `denoland/deno`
Question: For Node compatibility in Deno, what source files or tests establish the node:path module behavior covered by a broad parity smoke test: basename, dirname, extname, format, isAbsolute, join, normalize, parse, relative, resolve, toNamespacedPath, sep/delimiter, and the path.posix/path.win32 namespaces? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry test_parity_path known-failure entry.
Search: https://deepwiki.com/search/for-node-compatibility-in-deno_a2a232b8-5120-48b1-8661-e0643445567e

**Question**: For Node compatibility in Deno, what source files or tests establish the node:path module behavior covered by a broad parity smoke test: basename, dirname, extname, format, isAbsolute, join, normalize, parse, relative, resolve, toNamespacedPath, sep/delimiter, and the path.posix/path.win32 namespaces? Focus only on public source/tests useful as reference material for revalidating a stale PerryTS/perry test_parity_path known-failure entry.

The `node:path` module behavior in Deno for functions like `basename`, `dirname`, `extname`, `format`, `isAbsolute`, `join`, `normalize`, `parse`, `relative`, `resolve`, `toNamespacedPath`, `sep`/`delimiter`, and the `path.posix`/`path.win32` namespaces is primarily established by the TypeScript files located in `ext/node/polyfills/path/` [1](#ref-q1-1) . The core logic for POSIX-like paths is in `ext/node/polyfills/path/_posix.ts` [2](#ref-q1-2)  and for Windows-like paths in `ext/node/polyfills/path/_win32.ts` [3](#ref-q1-3) . These platform-specific implementations are then exposed through `ext/node/polyfills/path/mod.ts` [4](#ref-q1-4) , which dynamically selects the appropriate implementation based on the operating system [5](#ref-q1-5) .

## Source Files

The primary source files defining the `node:path` module behavior are:

*   **`ext/node/polyfills/path/mod.ts`**: This file acts as the main entry point for the `node:path` module [6](#ref-q1-6) . It imports and re-exports the platform-specific implementations from `_win32.ts` and `_posix.ts` [7](#ref-q1-7) . It also sets up the `path.posix` and `path.win32` namespaces [8](#ref-q1-8) .
*   **`ext/node/polyfills/path/_posix.ts`**: This file contains the implementation of all the specified `node:path` functions for POSIX systems [9](#ref-q1-9)  [10](#ref-q1-10)  [11](#ref-q1-11) . This includes `basename`, `dirname`, `extname`, `format`, `isAbsolute`, `join`, `normalize`, `parse`, `relative`, `resolve`, `toNamespacedPath`, `sep`, and `delimiter` [12](#ref-q1-12) .
*   **`ext/node/polyfills/path/_win32.ts`**: This file provides the Windows-specific implementations for the `node:path` functions [13](#ref-q1-13)  [14](#ref-q1-14) . It defines the `sep` as `\` and `delimiter` as `;` for Windows [3](#ref-q1-3) .
*   **`ext/node/polyfills/path.ts`**: This file re-exports the entire `node:path` module, making it available for use in Deno's Node.js compatibility layer [15](#ref-q1-15) .
*   **`ext/node/polyfills/path/posix.ts`**: This file specifically exports the POSIX implementation of the `node:path` module [16](#ref-q1-16) .
*   **`ext/node/polyfills/path/win32.ts`**: This file specifically exports the Windows implementation of the `node:path` module [17](#ref-q1-17) .
*   **`cli/tsc/dts/node/path.d.cts`**: This file contains the TypeScript declaration for the `node:path` module, outlining the expected API and types for all the functions you mentioned [18](#ref-q1-18) .

## Test Files

The Node.js compatibility tests for the `node:path` module are located in the `tests/node_compat/` directory [19](#ref-q1-19) .

*   **`tests/node_compat/config.jsonc`**: This configuration file lists the Node.js test cases that Deno runs for compatibility, including many related to `path` module functions [20](#ref-q1-20) . Specifically, entries like `"parallel/test-path-basename.js"`, `"parallel/test-path-dirname.js"`, `"parallel/test-path-extname.js"`, `"parallel/test-path-isabsolute.js"`, `"parallel/test-path-join.js"`, `"parallel/test-path-normalize.js"`, `"parallel/test-path-parse-format.js"`, `"parallel/test-path-relative.js"`, `"parallel/test-path-resolve.js"`, `"parallel/test-path-win32-normalize-device-names.js"`, and `"parallel/test-path-zero-length-strings.js"` directly cover the behavior of the `node:path` functions you listed [20](#ref-q1-20) .
*   **`tools/x.ts`**: This script defines the `test-compat` command, which is used to run the Node.js compatibility tests [21](#ref-q1-21) . You can use this command with a filter like `path` to run tests specifically for the `node:path` module [22](#ref-q1-22) .

## Notes

The `node:path` polyfill in Deno leverages internal Deno mechanisms to provide Node.js compatibility [23](#ref-q1-23) . The `_util.ts` file, loaded by both `_posix.ts` and `_win32.ts`, contains common utility functions used in path manipulation, such as `assertPath` and `normalizeString` [24](#ref-q1-24) . The `Releases.md` file also indicates ongoing work and fixes related to `node:path` compatibility, such as `fix(ext/node): fs.path' 'makelong' and 'resolve' compatibility` [25](#ref-q1-25)  and `fix(ext/node): path.normalize compatibility` [26](#ref-q1-26) .

Wiki pages you might want to explore:
- [File System Operations (denoland/deno)](/wiki/denoland/deno#3.4)
- [Process, TTY, and OS APIs (denoland/deno)](/wiki/denoland/deno#7.7)

View this search on DeepWiki: https://deepwiki.com/search/for-node-compatibility-in-deno_a2a232b8-5120-48b1-8661-e0643445567e

## References

<a id="ref-q1-1"></a>
### [1] `tools/core_import_map.json:661-664`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/core_import_map.json#L661-L664)

```json
    "ext:deno_node/path/_posix.ts": "../ext/node/polyfills/path/_posix.ts",
    "ext:deno_node/path/_util.ts": "../ext/node/polyfills/path/_util.ts",
    "ext:deno_node/path/_win32.ts": "../ext/node/polyfills/path/_win32.ts",
    "ext:deno_node/path/mod.ts": "../ext/node/polyfills/path/mod.ts",
```

<a id="ref-q1-2"></a>
### [2] `ext/node/polyfills/path/_posix.ts:36-37`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_posix.ts#L36-L37)

```typescript
const sep = "/";
const delimiter = ":";
```

<a id="ref-q1-3"></a>
### [3] `ext/node/polyfills/path/_win32.ts:46-47`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_win32.ts#L46-L47)

```typescript
const sep = "\\";
const delimiter = ";";
```

<a id="ref-q1-4"></a>
### [4] `ext/node/polyfills/path/mod.ts:26-43`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/mod.ts#L26-L43)

```typescript
const path = isWindows ? win32 : posix;
const {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
} = path;
```

<a id="ref-q1-5"></a>
### [5] `ext/node/polyfills/path/mod.ts:26-27`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/mod.ts#L26-L27)

```typescript
const path = isWindows ? win32 : posix;
const {
```

<a id="ref-q1-6"></a>
### [6] `ext/node/polyfills/path/mod.ts:4-67`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/mod.ts#L4-L67)

```typescript

(function () {
const { core } = globalThis.__bootstrap;
const { isWindows } = core.loadExtScript("ext:deno_node/_util/os.ts");
const _win32 = core.loadExtScript("ext:deno_node/path/_win32.ts").default;
const _posix = core.loadExtScript("ext:deno_node/path/_posix.ts").default;

const win32 = {
  ..._win32,
  win32: null,
  posix: null,
};

const posix = {
  ..._posix,
  win32: null,
  posix: null,
};

posix.win32 = win32.win32 = win32;
posix.posix = win32.posix = posix;

const path = isWindows ? win32 : posix;
const {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
} = path;
const { common } = core.loadExtScript("ext:deno_node/path/common.ts");

return {
  win32,
  posix,
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
  common,
  default: path,
};
})();
```

<a id="ref-q1-7"></a>
### [7] `ext/node/polyfills/path/mod.ts:8-9`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/mod.ts#L8-L9)

```typescript
const _win32 = core.loadExtScript("ext:deno_node/path/_win32.ts").default;
const _posix = core.loadExtScript("ext:deno_node/path/_posix.ts").default;
```

<a id="ref-q1-8"></a>
### [8] `ext/node/polyfills/path/mod.ts:11-24`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/mod.ts#L11-L24)

```typescript
const win32 = {
  ..._win32,
  win32: null,
  posix: null,
};

const posix = {
  ..._posix,
  win32: null,
  posix: null,
};

posix.win32 = win32.win32 = win32;
posix.posix = win32.posix = posix;
```

<a id="ref-q1-9"></a>
### [9] `ext/node/polyfills/path/_posix.ts:36-142`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_posix.ts#L36-L142)

```typescript
const sep = "/";
const delimiter = ":";

const posixCwd = (() => {
  if (isWindows) {
    // Converts Windows' backslash path separators to POSIX forward slashes
    // and truncates any drive indicator
    const regexp = new SafeRegExp(/\\/g);
    return () => {
      const cwd = StringPrototypeReplace(globalThis.process.cwd(), regexp, "/");
      return StringPrototypeSlice(cwd, StringPrototypeIndexOf(cwd, "/"));
    };
  }

  // We're already on POSIX, no need for any transformations
  return () => globalThis.process.cwd();
})();

// path.resolve([from ...], to)
/**
 * Resolves `pathSegments` into an absolute path.
 * @param pathSegments an array of path segments
 */
function resolve(...pathSegments: string[]): string {
  if (
    pathSegments.length === 0 ||
    (pathSegments.length === 1 &&
      (pathSegments[0] === "" || pathSegments[0] === "."))
  ) {
    const cwd = posixCwd();
    if (StringPrototypeCharCodeAt(cwd, 0) === CHAR_FORWARD_SLASH) {
      return cwd;
    }
  }
  let resolvedPath = "";
  let resolvedAbsolute = false;

  for (let i = pathSegments.length - 1; i >= 0 && !resolvedAbsolute; i--) {
    const path = pathSegments[i];
    validateString(path, `paths[${i}]`);

    // Skip empty entries
    if (path.length === 0) {
      continue;
    }

    resolvedPath = `${path}/${resolvedPath}`;
    resolvedAbsolute =
      StringPrototypeCharCodeAt(path, 0) === CHAR_FORWARD_SLASH;
  }

  if (!resolvedAbsolute) {
    const cwd = posixCwd();
    resolvedPath = `${cwd}/${resolvedPath}`;
    resolvedAbsolute = StringPrototypeCharCodeAt(cwd, 0) === CHAR_FORWARD_SLASH;
  }

  // At this point the path should be resolved to a full absolute path, but
  // handle relative paths to be safe (might happen when globalThis.process.cwd() fails)

  // Normalize the path
  resolvedPath = normalizeString(
    resolvedPath,
    !resolvedAbsolute,
    "/",
    isPosixPathSeparator,
  );

  if (resolvedAbsolute) {
    return `/${resolvedPath}`;
  }
  return resolvedPath.length > 0 ? resolvedPath : ".";
}

/**
 * Normalize the `path`, resolving `'..'` and `'.'` segments.
 * @param path to be normalized
 */
function normalize(path: string): string {
  assertPath(path);

  if (path.length === 0) return ".";

  const isAbsolute = StringPrototypeCharCodeAt(path, 0) === CHAR_FORWARD_SLASH;
  const trailingSeparator =
    StringPrototypeCharCodeAt(path, path.length - 1) === CHAR_FORWARD_SLASH;

  // Normalize the path
  path = normalizeString(path, !isAbsolute, "/", isPosixPathSeparator);

  if (path.length === 0 && !isAbsolute) path = ".";
  if (path.length > 0 && trailingSeparator) path += "/";

  if (isAbsolute) return `/${path}`;
  return path;
}

/**
 * Verifies whether provided path is absolute
 * @param path to be verified as absolute
 */
function isAbsolute(path: string): boolean {
  assertPath(path);
  return path.length > 0 &&
    StringPrototypeCharCodeAt(path, 0) === CHAR_FORWARD_SLASH;
}
```

<a id="ref-q1-10"></a>
### [10] `ext/node/polyfills/path/_posix.ts:240-275`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_posix.ts#L240-L275)

```typescript
/**
 * Resolves path to a namespace path
 * @param path to resolve to namespace
 */
function toNamespacedPath(path: string): string {
  // Non-op on posix systems
  return path;
}

/**
 * Return the directory name of a `path`.
 * @param path to determine name for
 */
function dirname(path: string): string {
  assertPath(path);
  if (path.length === 0) return ".";
  const hasRoot = StringPrototypeCharCodeAt(path, 0) === CHAR_FORWARD_SLASH;
  let end = -1;
  let matchedSlash = true;
  for (let i = path.length - 1; i >= 1; --i) {
    if (StringPrototypeCharCodeAt(path, i) === CHAR_FORWARD_SLASH) {
      if (!matchedSlash) {
        end = i;
        break;
      }
    } else {
      // We saw the first non-path separator
      matchedSlash = false;
    }
  }

  if (end === -1) return hasRoot ? "/" : ".";
  if (hasRoot && end === 1) return "//";
  return StringPrototypeSlice(path, 0, end);
}
```

<a id="ref-q1-11"></a>
### [11] `ext/node/polyfills/path/_posix.ts:508-552`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_posix.ts#L508-L552)

```typescript
const _makeLong = toNamespacedPath;

let lazyMatchGlobPattern: typeof fsGlob.matchGlobPattern;
const matchesGlob = (path: string, pattern: string): boolean => {
  lazyMatchGlobPattern ??= lazyLoadGlob().matchGlobPattern;
  return lazyMatchGlobPattern(path, pattern, false);
};

const _default = {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
};

return {
  sep,
  delimiter,
  resolve,
  normalize,
  isAbsolute,
  join,
  relative,
  toNamespacedPath,
  dirname,
  basename,
  extname,
  format,
  parse,
  _makeLong,
  matchesGlob,
  default: _default,
};
})();
```

<a id="ref-q1-12"></a>
### [12] `ext/node/polyfills/path/_posix.ts:517-529`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_posix.ts#L517-L529)

```typescript
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
```

<a id="ref-q1-13"></a>
### [13] `ext/node/polyfills/path/_win32.ts:1-92`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_win32.ts#L1-L92)

```typescript
// Copyright the Browserify authors. MIT License.
// Ported from https://github.com/browserify/path-browserify/
// Copyright 2018-2026 the Deno authors. MIT license.

(function () {
const { core, primordials } = globalThis.__bootstrap;
const {
  CHAR_BACKWARD_SLASH,
  CHAR_COLON,
  CHAR_DOT,
  CHAR_QUESTION_MARK,
} = core.loadExtScript("ext:deno_node/path/_constants.ts");
const { ERR_INVALID_ARG_TYPE } = core.loadExtScript(
  "ext:deno_node/internal/errors.ts",
);

const {
  _format,
  assertPath,
  isPathSeparator,
  isPosixPathSeparator,
  isWindowsDeviceRoot,
  normalizeString,
} = core.loadExtScript("ext:deno_node/path/_util.ts");
const { default: assert } = core.loadExtScript("ext:deno_node/assert.ts");
const lazyLoadGlob = core.createLazyLoader(
  "ext:deno_node/_fs/_fs_glob.ts",
);

const {
  ArrayPrototypeIncludes,
  ArrayPrototypeJoin,
  ArrayPrototypePop,
  ArrayPrototypeSlice,
  StringPrototypeCharCodeAt,
  StringPrototypeIncludes,
  StringPrototypeIndexOf,
  StringPrototypeRepeat,
  StringPrototypeSlice,
  StringPrototypeSplit,
  StringPrototypeToLowerCase,
  StringPrototypeToUpperCase,
  TypeError,
} = primordials;

const sep = "\\";
const delimiter = ";";

const WINDOWS_RESERVED_NAMES = [
  "CON",
  "PRN",
  "AUX",
  "NUL",
  "COM1",
  "COM2",
  "COM3",
  "COM4",
  "COM5",
  "COM6",
  "COM7",
  "COM8",
  "COM9",
  "LPT1",
  "LPT2",
  "LPT3",
  "LPT4",
  "LPT5",
  "LPT6",
  "LPT7",
  "LPT8",
  "LPT9",
  "COM\xb9",
  "COM\xb2",
  "COM\xb3",
  "LPT\xb9",
  "LPT\xb2",
  "LPT\xb3",
];

function isWindowsReservedName(path: string, colonIndex: number): boolean {
  const devicePart = StringPrototypeToUpperCase(
    StringPrototypeSlice(path, 0, colonIndex),
  );
  return ArrayPrototypeIncludes(WINDOWS_RESERVED_NAMES, devicePart);
}

/**
 * Resolves path segments into a `path`
 * @param pathSegments to process to path
 */
function resolve(...pathSegments: string[]): string {
  let resolvedDevice = "";
```

<a id="ref-q1-14"></a>
### [14] `ext/node/polyfills/path/_win32.ts:1189-1233`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_win32.ts#L1189-L1233)

```typescript
const _makeLong = toNamespacedPath;

let lazyMatchGlobPattern: typeof fsGlob.matchGlobPattern;
const matchesGlob = (path: string, pattern: string): boolean => {
  lazyMatchGlobPattern ??= lazyLoadGlob().matchGlobPattern;
  return lazyMatchGlobPattern(path, pattern, true);
};

const _default = {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
};

return {
  sep,
  delimiter,
  resolve,
  normalize,
  isAbsolute,
  join,
  relative,
  toNamespacedPath,
  dirname,
  basename,
  extname,
  format,
  parse,
  _makeLong,
  matchesGlob,
  default: _default,
};
})();
```

<a id="ref-q1-15"></a>
### [15] `ext/node/polyfills/path.ts:6-27`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path.ts#L6-L27)

```typescript
export const {
  win32,
  posix,
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
  matchesGlob,
  common,
} = mod;

export default mod.default;
```

<a id="ref-q1-16"></a>
### [16] `ext/node/polyfills/path/posix.ts:8-27`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/posix.ts#L8-L27)

```typescript
export const {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
} = mod.posix;

export const posix = mod.posix;
export const win32 = mod.win32;
export default mod.posix;
```

<a id="ref-q1-17"></a>
### [17] `ext/node/polyfills/path/win32.ts:8-27`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/win32.ts#L8-L27)

```typescript
export const {
  basename,
  delimiter,
  dirname,
  extname,
  format,
  isAbsolute,
  join,
  normalize,
  parse,
  relative,
  resolve,
  sep,
  toNamespacedPath,
  _makeLong,
} = mod.win32;

export const posix = mod.posix;
export const win32 = mod.win32;
export default mod.win32;
```

<a id="ref-q1-18"></a>
### [18] `cli/tsc/dts/node/path.d.cts:121-183`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/cli/tsc/dts/node/path.d.cts#L121-L183)

```
            relative(from: string, to: string): string;
            /**
             * Return the directory name of a path. Similar to the Unix dirname command.
             *
             * @param path the path to evaluate.
             * @throws {TypeError} if `path` is not a string.
             */
            dirname(path: string): string;
            /**
             * Return the last portion of a path. Similar to the Unix basename command.
             * Often used to extract the file name from a fully qualified path.
             *
             * @param path the path to evaluate.
             * @param suffix optionally, an extension to remove from the result.
             * @throws {TypeError} if `path` is not a string or if `ext` is given and is not a string.
             */
            basename(path: string, suffix?: string): string;
            /**
             * Return the extension of the path, from the last '.' to end of string in the last portion of the path.
             * If there is no '.' in the last portion of the path or the first character of it is '.', then it returns an empty string.
             *
             * @param path the path to evaluate.
             * @throws {TypeError} if `path` is not a string.
             */
            extname(path: string): string;
            /**
             * The platform-specific file separator. '\\' or '/'.
             */
            readonly sep: "\\" | "/";
            /**
             * The platform-specific file delimiter. ';' or ':'.
             */
            readonly delimiter: ";" | ":";
            /**
             * Returns an object from a path string - the opposite of format().
             *
             * @param path path to evaluate.
             * @throws {TypeError} if `path` is not a string.
             */
            parse(path: string): ParsedPath;
            /**
             * Returns a path string from an object - the opposite of parse().
             *
             * @param pathObject path to evaluate.
             */
            format(pathObject: FormatInputPathObject): string;
            /**
             * On Windows systems only, returns an equivalent namespace-prefixed path for the given path.
             * If path is not a string, path will be returned without modifications.
             * This method is meaningful only on Windows system.
             * On POSIX systems, the method is non-operational and always returns path without modifications.
             */
            toNamespacedPath(path: string): string;
            /**
             * Posix specific pathing.
             * Same as parent object on posix.
             */
            readonly posix: PlatformPath;
            /**
             * Windows specific pathing.
             * Same as parent object on windows
             */
            readonly win32: PlatformPath;
```

<a id="ref-q1-19"></a>
### [19] `tests/node_compat/README.md:1-3`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/README.md#L1-L3)

```markdown
# Node compat test directory

This directory includes the tools for running Node.js test cases directly in
```

<a id="ref-q1-20"></a>
### [20] `tests/node_compat/config.jsonc:2563-2579`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tests/node_compat/config.jsonc#L2563-L2579)

```
    "parallel/test-path-basename.js": {},
    "parallel/test-path-dirname.js": {},
    "parallel/test-path-extname.js": {},
    "parallel/test-path-glob.js": {},
    "parallel/test-path-isabsolute.js": {},
    "parallel/test-path-join.js": {},
    "parallel/test-path-makelong.js": {},
    "parallel/test-path-normalize.js": {},
    "parallel/test-path-parse-format.js": {},
    "parallel/test-path-posix-exists.js": {},
    "parallel/test-path-posix-relative-on-windows.js": {},
    "parallel/test-path-relative.js": {},
    "parallel/test-path-resolve.js": {},
    "parallel/test-path-win32-exists.js": {},
    "parallel/test-path-win32-normalize-device-names.js": {},
    "parallel/test-path-zero-length-strings.js": {},
    "parallel/test-path.js": {},
```

<a id="ref-q1-21"></a>
### [21] `tools/x.ts:218-240`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/x.ts#L218-L240)

```typescript
    "test-compat": cargoTestCommand(root, ["--test", "node_compat"], {
      description: "Run Node.js compatibility tests",
      stepName: "Node.js compatibility tests",
      help: `Runs the Node.js compatibility test suite. These tests use actual
Node.js test cases (ported or adapted) to verify that Deno's node:*
module implementations match Node.js behavior.

The test runner lives in tests/node_compat/runner/.

Requires a filter argument to select which tests to run. The filter is
a substring match against test names.

Usage:
  ./x test-compat <filter>  Run tests matching the filter
  ./x test-compat --list    List all available tests

Examples:
  ./x test-compat fs        Run tests with "fs" in their name
  ./x test-compat path      Run tests with "path" in their name

Under the hood:
  cargo test --test node_compat -- <filter>`,
    }),
```

<a id="ref-q1-22"></a>
### [22] `tools/x.ts:236-237`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/tools/x.ts#L236-L237)

```typescript
  ./x test-compat path      Run tests with "path" in their name
```

<a id="ref-q1-23"></a>
### [23] `File System Operations:1`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/File System Operations#L1)

<a id="ref-q1-24"></a>
### [24] `ext/node/polyfills/path/_win32.ts:18-24`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/ext/node/polyfills/path/_win32.ts#L18-L24)

```typescript
  _format,
  assertPath,
  isPathSeparator,
  isPosixPathSeparator,
  isWindowsDeviceRoot,
  normalizeString,
} = core.loadExtScript("ext:deno_node/path/_util.ts");
```

<a id="ref-q1-25"></a>
### [25] `Releases.md:1473`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/Releases.md#L1473)

```markdown
- fix(ext/node): `fs.path`' `makelong` and `resolve` compatibility (#30503)
```

<a id="ref-q1-26"></a>
### [26] `Releases.md:1481`
Source: [denoland/deno @ d6212d40](https://github.com/denoland/deno/blob/d6212d40/Releases.md#L1481)

```markdown
- fix(ext/node): path.normalize compatibility (#30537)
```

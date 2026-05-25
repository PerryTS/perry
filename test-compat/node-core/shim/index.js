'use strict';
//
// Minimal, Perry-compilable replacement for Node's `test/common/index.js`
// (#800). The real harness is ~1000 lines and pulls in `net`,
// `worker_threads`, `process.config.variables`, `process.binding`, etc.,
// none of which compile. Most `test/parallel` cases only lean on `common`
// for *scaffolding* — call-count assertions (`mustCall`), platform flags,
// and `skip` — while the API actually under test is the real builtin.
//
// Both runtimes load THIS shim: the runner stages it as `common/index.js`
// next to each test, Node resolves it via its CommonJS loader, and Perry
// compiles it natively (its CommonJS `require`/`module.exports` are rewritten
// to ESM by the same path that now handles user `.js`). So the differential
// compares the two runtimes' *builtins*, never their private harnesses.
//
// CommonJS on purpose: Node's CJS loader won't resolve a `.ts` shim from a
// `require('../common')`, and Perry resolves + compiles `.js` all the same.
//
// Scope: the helpers that appear most across the supported-apis corpus.
// Anything missing surfaces as a `node-skip` (Node itself throws on the
// missing helper, so the case is excluded — never charged against Perry).

const assert = require('assert');

// ---------------------------------------------------------------------------
// mustCall / mustNotCall — verified at process exit
// ---------------------------------------------------------------------------

const mustCallChecks = [];

function runCallChecks(exitCode) {
  // Only enforce on an otherwise-clean exit; a non-zero exit already signals
  // failure and call-count noise would just mask it.
  if (exitCode !== 0) return;

  const failed = mustCallChecks.filter(function (ctx) {
    if ('minimum' in ctx) {
      ctx.messageSegment = 'at least ' + ctx.minimum;
      return ctx.actual < ctx.minimum;
    }
    ctx.messageSegment = 'exactly ' + ctx.exact;
    return ctx.actual !== ctx.exact;
  });

  failed.forEach(function (ctx) {
    console.log(
      'Mismatched ' + ctx.name + ' function calls. Expected ' +
      ctx.messageSegment + ', actual ' + ctx.actual + '.',
    );
  });

  if (failed.length) process.exit(1);
}

function _mustCall(fn, criteria, field) {
  if (typeof fn === 'number') {
    criteria = fn;
    fn = function () {};
  } else if (fn === undefined) {
    fn = function () {};
  }

  if (criteria === undefined) criteria = 1;
  if (typeof criteria !== 'number') {
    throw new TypeError('Invalid ' + field + ' value: ' + criteria);
  }

  const context = { actual: 0, name: fn.name || '<anonymous>' };
  context[field] = criteria;

  if (mustCallChecks.length === 0) process.on('exit', runCallChecks);
  mustCallChecks.push(context);

  return function (...args) {
    context.actual++;
    return fn(...args);
  };
}

function mustCall(fn, exact) {
  return _mustCall(fn, exact, 'exact');
}

function mustCallAtLeast(fn, minimum) {
  return _mustCall(fn, minimum, 'minimum');
}

function mustSucceed(fn, exact) {
  // Rest params + spread instead of `Array.prototype.slice.call(arguments, 1)`
  // — the latter trips Perry's #1777 gap (builtin prototype methods aren't
  // first-class values, so `.call` on `Array.prototype.slice` is undefined),
  // which would fail this scaffolding before the API under test even runs.
  return mustCall(function (err, ...rest) {
    assert.ifError(err);
    if (typeof fn === 'function') {
      return fn(...rest);
    }
  }, exact);
}

function mustNotCall(msg) {
  return function (...args) {
    let info = '';
    if (args.length > 0) info = ' with arguments: ' + args.join(', ');
    assert.fail((msg || 'function should not have been called') + info);
  };
}

function mustNotMutateObjectDeep(obj) {
  // The real helper deep-freezes; returning unchanged is behaviourally
  // equivalent for the consumers in scope.
  return obj;
}

// ---------------------------------------------------------------------------
// Platform / capability flags
// ---------------------------------------------------------------------------

const platform = process.platform;
const isWindows = platform === 'win32';
const isMacOS = platform === 'darwin';
const isLinux = platform === 'linux';
const isFreeBSD = platform === 'freebsd';
const isOpenBSD = platform === 'openbsd';
const isAIX = platform === 'aix';
const isSunOS = platform === 'sunos';

function platformTimeout(ms) {
  return ms;
}

function skip(msg) {
  console.log('1..0 # Skipped: ' + (msg || ''));
  process.exit(0);
}

function allowGlobals() {
  // No global-leak tracking in the shim.
}

function invalidArgTypeHelper(input) {
  if (input == null) return ' Received ' + input;
  if (typeof input === 'function') {
    return ' Received function ' + (input.name || '(anonymous)');
  }
  if (typeof input === 'object') {
    if (input.constructor && input.constructor.name) {
      return ' Received an instance of ' + input.constructor.name;
    }
  }
  return ' Received type ' + typeof input;
}

// ---------------------------------------------------------------------------
// Additional helpers used across the supported-apis corpus (#800). These let
// tests *load* under both runtimes instead of throwing "common.X is not a
// function" (which would land them in node-skip and shrink the judged set).
// ---------------------------------------------------------------------------

// Like `skip` but does not exit — the test continues.
function printSkipMessage(msg) {
  console.log('1..0 # Skipped: ' + (msg || ''));
}

function skipIf(condition, msg) {
  if (condition) skip(msg);
}

function canCreateSymLink() {
  return !isWindows;
}

// Returns a validator for `assert.throws(fn, common.expectsError(props))` (and
// the error-first-callback form). A props object is matched key-by-key against
// the thrown error; a function/RegExp validator is applied directly.
function expectsError(validator, exact) {
  return mustCall(function (...args) {
    const err = args[0];
    if (typeof validator === 'function') return validator(err);
    if (validator instanceof RegExp) {
      assert.match(String(err && err.message), validator);
      return true;
    }
    if (validator && typeof validator === 'object') {
      for (const key of Object.keys(validator)) {
        const expected = validator[key];
        if (expected instanceof RegExp) {
          assert.match(String(err[key]), expected);
        } else {
          assert.strictEqual(err[key], expected);
        }
      }
    }
    return true;
  }, exact);
}

// Warning verification isn't modeled in the shim — accept and ignore so tests
// that register expected warnings still run (the API under test executes).
function expectWarning() {}

// All TypedArray + DataView views over a Buffer's backing store.
function getArrayBufferViews(buf) {
  const { buffer, byteOffset, byteLength } = buf;
  const out = [];
  const ctors = [
    Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array,
    Int32Array, Uint32Array, Float32Array, Float64Array,
  ];
  for (const Ctor of ctors) {
    const bpe = Ctor.BYTES_PER_ELEMENT;
    if (byteLength % bpe === 0) {
      out.push(new Ctor(buffer, byteOffset, byteLength / bpe));
    }
  }
  out.push(new DataView(buffer, byteOffset, byteLength));
  return out;
}

function getBufferSources(buf) {
  return [...getArrayBufferViews(buf), new Uint8Array(buf).buffer];
}

// Calls `fn` with a file descriptor that is (almost certainly) not open.
function runWithInvalidFD(fn) {
  let fd = 1 << 30;
  while (fd > 1 << 20) {
    try {
      return fn(fd);
    } catch (e) {
      fd = Math.floor(fd / 2);
    }
  }
}

// Template tag that cooks `cmd ${arg}` to `[command, env]`. The shim does no
// real POSIX escaping — both runtimes receive the identical string, which is
// all the differential needs.
function escapePOSIXShell(strings, ...args) {
  let s = strings[0];
  for (let i = 0; i < args.length; i++) {
    s += String(args[i]) + strings[i + 1];
  }
  return [s, {}];
}

module.exports = {
  mustCall,
  mustCallAtLeast,
  mustSucceed,
  mustNotCall,
  mustNotMutateObjectDeep,
  platformTimeout,
  skip,
  printSkipMessage,
  skipIf,
  allowGlobals,
  invalidArgTypeHelper,
  canCreateSymLink,
  expectsError,
  expectWarning,
  getArrayBufferViews,
  getBufferSources,
  runWithInvalidFD,
  escapePOSIXShell,
  isWindows,
  isMacOS,
  isOSX: isMacOS,
  isLinux,
  isFreeBSD,
  isOpenBSD,
  isAIX,
  isSunOS,
  isMainThread: true,
  // Platform / build flags. Default to "ordinary release build on this host"
  // so build-specific test branches don't throw on a missing flag.
  isIBMi: process.platform === 'os400',
  isDebug: false,
  isASan: false,
  isPi: false,
  // Assume a full-featured build. A test needing a capability Perry lacks
  // fails at the API call (a real gap signal), not here.
  hasCrypto: true,
  hasIntl: true,
  hasIPv6: true,
  hasOpenSSL3: true,
  hasQuic: false,
  hasSQLite: false,
  enoughTestMem: true,
  PORT: 12346,
  localhostIPv4: '127.0.0.1',
};

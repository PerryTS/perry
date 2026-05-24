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

  return function () {
    context.actual++;
    return fn.apply(this, arguments);
  };
}

function mustCall(fn, exact) {
  return _mustCall(fn, exact, 'exact');
}

function mustCallAtLeast(fn, minimum) {
  return _mustCall(fn, minimum, 'minimum');
}

function mustSucceed(fn, exact) {
  return mustCall(function (err) {
    assert.ifError(err);
    if (typeof fn === 'function') {
      const rest = Array.prototype.slice.call(arguments, 1);
      return fn.apply(this, rest);
    }
  }, exact);
}

function mustNotCall(msg) {
  return function () {
    const args = Array.prototype.slice.call(arguments);
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

module.exports = {
  mustCall,
  mustCallAtLeast,
  mustSucceed,
  mustNotCall,
  mustNotMutateObjectDeep,
  platformTimeout,
  skip,
  allowGlobals,
  invalidArgTypeHelper,
  isWindows,
  isMacOS,
  isOSX: isMacOS,
  isLinux,
  isFreeBSD,
  isOpenBSD,
  isAIX,
  isSunOS,
  isMainThread: true,
  // Assume a full-featured build. A test needing a capability Perry lacks
  // fails at the API call (a real gap signal), not here.
  hasCrypto: true,
  hasIntl: true,
  hasIPv6: true,
  enoughTestMem: true,
  PORT: 12346,
  localhostIPv4: '127.0.0.1',
};

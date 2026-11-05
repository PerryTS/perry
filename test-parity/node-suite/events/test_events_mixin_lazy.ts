// Regression test for #984-followup: events shim must lazy-init `_events`
// so that mixin patterns (e.g. express's createApplication which copies
// EventEmitter.prototype methods onto an `app` object without invoking the
// constructor) don't crash with "Cannot read properties of undefined".
//
// Node's real lib/events.js does `this._events ??= ObjectCreate(null)` in
// every method; the V8-fallback shim now matches that behavior.
//
// We can't reliably test the cross-boundary mixin pattern here because
// Object.create / Object.assign between V8 handles and native-side objects
// doesn't preserve prototype methods today (separate gap). The real
// regression we close is in the shim itself — verified by the express
// smoke test under test-parity / by the explicit `app.on('mount', ...)`
// crash no longer firing when express runs end-to-end.
import { EventEmitter } from 'node:events';

const ee = new EventEmitter();
ee.on('a', () => console.log('a-fired'));
ee.emit('a');

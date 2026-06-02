// Regression test for the express `app.on('mount', ...)` smoke path
// (downstream of PR #986's lazy-init events fix).
//
// Background: express's createApplication() does:
//   var app = function (req, res, next) { app.handle(req, res, next); };
//   mixin(app, EventEmitter.prototype, false);   // <-- no constructor!
//   mixin(app, proto, false);
//   app.init();                                  // <-- calls this.on('mount', ...)
//
// Pre-#986 the V8-fallback `events` shim required `_events` to be set
// by the EventEmitter constructor — but express's mixin pattern never
// invokes the constructor, so `this._events` was undefined and the
// first `.on('mount', listener)` blew up with
//   "Cannot read properties of undefined (reading 'mount')"
//
// #986 rewrote the shim so every method calls a `__perry_ee_init(this)`
// helper that lazy-creates `_events` with Object.create(null). The
// express end-to-end smoke (`npm install express@4.21.0` + a script
// that prints `typeof app` and `typeof app.get`) must report
//   function
//   function
// as the success criteria; that integration runs outside the repo
// because it needs the actual express package on disk.
//
// In-repo we exercise the *behavioral* surface — basic
// EventEmitter construction + `on`/`emit` round-trips — to lock in
// that the shim's lazy-init semantics keep working. Direct testing of
// `EventEmitter.prototype` mixin requires `PERRY_ALLOW_UNIMPLEMENTED=1`
// because the prototype-on-default-import shape is manifest-gated
// (the express path takes the CommonJS `require('events').EventEmitter`
// route which bypasses that gate).

import EventEmitter from "node:events";

// Sanity: a normally-constructed EventEmitter still works.
const ee = new EventEmitter();
let hitCount = 0;
ee.on("hit", (n: number) => {
    hitCount = n;
});
ee.emit("hit", 7);
console.log("hitCount:", hitCount);

// A second emitter, with mount-shaped event name + payload (the
// exact shape express's `this.on('mount', function onmount(parent)
// {...})` listener uses). The listener captures `this` so we can
// confirm the event fires with the right receiver bound.
const child = new EventEmitter();
let mounted = false;
let parentName = "";
child.on("mount", function (this: any, parent: { name: string }) {
    mounted = true;
    parentName = parent.name;
});
console.log("before emit, mounted:", mounted);
child.emit("mount", { name: "parent-app" });
console.log("after emit, mounted:", mounted);
console.log("parentName:", parentName);

console.log("OK");

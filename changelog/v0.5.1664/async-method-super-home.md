`super.m()` and `super.x` inside an async class method after an `await` now
resolve against the class's base. An async method with an `await` runs its
body in a state-machine step closure; its home binding was evaluated there and
named the step closure, so the lookup missed and the call threw "value is not
a function". The home binding is now lifted into the outer wrapper like a
generator method's (only the home; async parameter binding still rejects the
promise). Next.js's `NextNodeServer#prepareImpl` (`await super.prepareImpl()`)
hit this on every request.

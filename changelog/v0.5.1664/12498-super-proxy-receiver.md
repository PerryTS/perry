fix(runtime): preserve receiver for proxy-backed super writes

A Proxy in a `super` assignment's live prototype chain now receives the original `this` as the fourth `set` trap argument. Successful writes therefore land on the receiver, and strict failure behavior continues through the shared PutValue path.

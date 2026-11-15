A one-expression arrow declared in a loop, block or branch of module code and
only called there (`for (...) { const p = new P(i); const f = () => p.x; s +=
f(); }`) is now reduced to its body at each call, as it already was inside a
function, so the closure and the object it captures no longer escape to the
heap. The top-level module statements, whose bindings are module globals, are
left as they are, and so is a module with top-level await. A closure that is
also used from a function declared in the loop, or in any other way than a
direct call, keeps its closure.

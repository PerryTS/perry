`{ ...Object.preventExtensions(o) }` is an extensible object, and `t.prototype = v` evaluates to `v` under whole-program facts, so `assign(42, o) === o` holds for a non-function receiver.

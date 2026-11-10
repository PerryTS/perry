wpfacts: a `String.prototype` method called with an unknown (⊤) receiver
(`String.prototype.replace.call(top, re, v)`, three.js `sanitizeNodeName` on
an unknown name) runs ToString(this), ⊤'s own code that may throw, and then
the string method's model. Its arguments are no longer handed to an opaque
builtin callee, and its result is the method's (a string), not ⊤.

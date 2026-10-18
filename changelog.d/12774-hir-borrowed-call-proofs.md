Fresh ordinary-function IIFE .call folds now retain one source function and
prove Function.prototype.call before arguments run. Whole-program facts omit
the prototype test when compiled and dynamic code cannot replace the method;
otherwise a failed Pristine layout-word check invokes the complete original
call. Discarded thisArg expressions retain their effects. Other borrowed-call
rewrites that lack the required source/wrapper identity proof now use ordinary
checked method dispatch, including native modules, namespace statics, builtin
prototype methods, captured aliases, immediate bind, and async/generator kinds.

Object constructor no longer installs own copies of inherited Object.prototype
methods: those copies hid prototype replacement from reflective borrowed calls
and gave the methods different function identities. Added dynamic-code sabotage,
own-wrapper, array-like apply, lexical-shadow, function-kind, and evaluation-order
parity coverage plus a runtime prototype identity/mutation test.

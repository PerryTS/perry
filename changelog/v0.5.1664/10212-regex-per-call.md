Made each `RegExp.prototype.test` and `exec` call cheaper. When a RegExp, its
prototype and `exec` are the untouched builtins, the unobservable `exec`
property lookup is skipped. Match scratch for programs of up to 32 registers
and 16 captures is held inline instead of heap-allocated per call, and
captures of an ASCII string are copied as one byte range instead of being
decoded and re-encoded twice.

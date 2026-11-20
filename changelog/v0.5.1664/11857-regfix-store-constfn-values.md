A static-key store site whose value can never be a closure (a literal, an
operator's primitive result, an object or array literal) no longer emits the
ConstFn lane admission (#11798) on its hit and key-add paths: a flagged lane
takes the miss, exactly as a failed admission does. A store of `v + 1` or of a
string literal is back to its size before the ConstFn lanes (957 and 861
bytes against 1,311 and 1,208), which most of TypeScript's enum and
initializer stores are.

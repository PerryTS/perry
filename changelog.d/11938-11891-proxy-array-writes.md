Proxy element writes through `T[]` parameters now run the Proxy's `[[Set]]`
operation instead of trusting the declared element type as an array-layout
fact. Counter-loop and straight-line writes now invoke `set` traps, forward to
the target when no trap exists, and throw on a falsy trap result in strict
code, matching Node.

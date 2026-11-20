Compiled code is smaller where a cold path makes runtime calls. A method call
on an untyped receiver now has one cold call instead of two (a primitive
receiver goes through the same miss entry, which forwards it to the generic
dispatch); the raw-f64 element-store arm makes one runtime call instead of
two; and a `catch` entry makes one call instead of three. Hot paths emit the
same instructions as before. The claude-code bundle's compiled JavaScript is
2.9 MB (1.1%) smaller, the TypeScript compiler's 0.5 MB.

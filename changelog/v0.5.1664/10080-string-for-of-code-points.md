Fix typed string `for...of` loops splitting astral characters into separate
surrogate iterations. Both function-body and module-initialization lowering
now convert strings to code-point arrays using the runtime string iterator's
existing WTF-8 conversion, preserving lone surrogates and the code-unit
semantics of bracket indexing and `charCodeAt`.

Regression coverage checks every yielded code unit for typed and dynamic
strings, local and module-level loops, adjacent astral characters, lone
surrogates, empty/ASCII strings, assignment heads, `break`, `continue`,
`return`, and `for await...of`.

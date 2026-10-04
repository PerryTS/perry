Split four source files back under the 2000-line cap: the node-module namespace
constructors, the array Proxy element tests, two codegen test modules and the
`JsFunctionInfo` fact recording each move to their own file. No behavior change.

Codegen: an untyped binding that is grown with `+=` and assigned a string
literal (the `var output; ... output = ""; ... output += s` writer shape in
TypeScript itself) now takes the in-place string append, and reading such a
binding's `.length` between appends no longer forces the next append to copy
it. On a three-transpile `ts.transpileModule` workload this removes a
quadratic 6.5 GB string build: full collections drop from 237 to 1,
instructions by 85% and peak RSS by 25%.

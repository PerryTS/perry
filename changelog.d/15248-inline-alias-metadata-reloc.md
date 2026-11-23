`perry compile` no longer panics with `alias metadata operand: ParseIntError
{ kind: Empty }` when callable lowering runs in parallel over a module with an
indexed owner loop: relocation leaves inline `!alias.scope !{...}` and
`!noalias !{...}` operands unchanged and renumbers only numbered ones.

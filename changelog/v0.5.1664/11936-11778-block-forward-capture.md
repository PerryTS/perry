Fixed closures in module-level blocks resolving a later `let`, `const`, or
class declaration as an unresolved global. Block entry now establishes the
lexical capture cells and forward class bindings before lowering closures, so
calls made after declaration match JavaScript lexical-environment semantics.

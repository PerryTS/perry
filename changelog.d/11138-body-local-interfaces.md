Preserve body-local interface types during HIR lowering (#11138). Interface
receivers now call their own methods instead of being mistaken for arrays when
a method is named `push`. Generic interface receivers use the same dispatch
classification as non-generic interfaces.

Register interfaces before lowering their lexical scope, including forward
references, function-expression bodies, and the shared scope of switch cases.
Restore the enclosing interface metadata afterward so local declarations do not
leak into sibling scopes or overwrite outer JSON.parse shape hints.

Add unit coverage for declaration order, generic interfaces, nested functions,
arrows, function expressions, async/generator functions, methods, blocks,
conditionals, loops, try blocks, switch cases, and asserted receivers. Keep a
positive ArrayPush control and a regression fixture for native/Node parity.

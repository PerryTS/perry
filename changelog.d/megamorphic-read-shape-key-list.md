Megamorphic property reads (a site that has seen more shapes than its inline
cache holds, such as `node.kind` across a compiler AST) are answered from the
receiver's own shape key list instead of the generic miss handler. A 40-shape
`o.kind` read drops from ~730 to ~315 instructions.

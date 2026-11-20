A compound or logical member assignment used as a value — `(levels[depth++] ??=
new Set()).add(x)`, `const v = (arr[k++] += 1)`, `(getObj().n ||= 1)` — evaluates
its base and computed key once. Only the statement form was fixed (#6071); the
expression form ran `depth++` twice, leaving holes in the array. Targets whose
base and key have no side effects keep their existing lowering.

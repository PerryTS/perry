Named class expressions now preserve the lexical identity of their self-binding
for each evaluation. Static private state no longer leaks between factory calls,
`call`/`apply` cannot redirect lexical self-access to another class, and nested
classes can close over an outer evaluated class's static private members.

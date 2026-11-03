Array element stores (`xs[i] = v`) now check one GC header word for the whole
structural guard, including frozen, sealed and non-extensible arrays, and
consult the prototype chain only when the store adds an element (a hole or an
index at or past `length`). A numeric array takes a Number with its NaN
canonicalized inline, and a non-Number clears the array's numeric layout
before the value is written. On `xs[k & 7] = v` loops the store drops by
20 to 70 instructions.

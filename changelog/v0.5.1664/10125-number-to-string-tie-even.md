Fix decimal number-to-string conversion to use ECMAScript's round-half-to-even
tie rule. `String`, template literals, `Number.prototype.toString` and
`toPrecision`, array joins, implicit concatenation, and JSON serialization now
agree on the shortest representation for exact-tie doubles.

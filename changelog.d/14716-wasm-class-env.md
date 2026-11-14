On the web target a class expression that captures locals keeps each
evaluation's captures: `function mk(tag) { return class { who() { return tag; } }; }`
gives classes whose methods answer for their own `tag`, including through
`A.prototype.who.call(b)` and `Reflect.apply`.

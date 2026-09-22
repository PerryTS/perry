Fixed runtime iteration of an inline anonymous class expression constructed
with `new (class { ... })()`. Computed methods such as generator
`[Symbol.iterator]` are now registered before the instance is constructed,
matching ordinary class expressions.

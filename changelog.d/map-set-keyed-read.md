A keyed read `m[k]` on a Map or Set with a key computed at run time read the
collection's storage as an array: `map[0]` answered the first entry key and
every other key (`map["marker"]`, `map["get"]`, a subclass accessor) answered
`undefined`. It is now the ordinary property read: own properties, then the
prototype chain.

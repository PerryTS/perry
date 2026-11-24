On `--target web`, the getters and setters of an object literal run, as in Node: a read calls the getter, a write calls
the setter, and both see the receiver as `this`.

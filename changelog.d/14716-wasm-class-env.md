On `--target web`, each evaluation of a class expression that captures locals is its
own class with its own captures, so `function mk(tag) { return class { who() { return tag; } }; }`
no longer evaluates to `undefined`. A class method's `super.x` read walks the parent
chain, and throws under `extends null`.

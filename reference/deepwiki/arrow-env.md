# Arrow Environment Parity Notes

DeepWiki research was captured for:

- `engine262/engine262`: `reference/deepwiki/arrow-env-engine262.md`
- `boa-dev/boa`: `reference/deepwiki/arrow-env-boa.md`

Useful reference behavior for the c262 arrow-function environment bucket:

- Arrow functions are ordinary ECMAScript functions with lexical `this` mode. They do not allocate their own `arguments`, `this`, or `super` binding; `super` resolves through the enclosing non-arrow function environment.
- Default parameter initializers run during function declaration instantiation while parameter bindings are still being initialized. A parameter default that reads its own binding, such as `(x = x) => {}`, observes the uninitialized binding and throws `ReferenceError`.
- When a non-strict function has parameter expressions, the spec creates a distinct parameter evaluation environment, parameter binding environment, body `var` environment, and body lexical environment. Closures created in parameter defaults must not capture body `var` declarations.
- Sloppy direct `eval` in parameter initializers can introduce `var` bindings into the parameter/callee environment, not the later body `var` environment. Parameter-default closures and later parameter eval closures should see that shared binding.
- Body-level sloppy direct `eval("var x;")` must throw `SyntaxError` if the introduced `var` conflicts with a top-level lexical declaration in the function body.
- Early errors reject duplicate/conflicting lexical and body names for arrow concise bodies, while non-strict arrows remain allowed unless their body is strict code or another early-error rule applies.

Implementation anchors from the reference engines:

- engine262 models this in `FunctionDeclarationInstantiation`, including the special `[[ThisMode]] === lexical` path, the no-`arguments` rule for arrows, parameter-expression environment splitting, and body `var` vs lexical environment separation.
- Boa mirrors the same structure in scope analysis: arrow functions enter `visit_function_like`, build `FunctionScopes`, optionally create `parameters_eval_scope` for parameter expressions, and visit parameters and body under separate scopes. Boa also tracks arrow context so `this`/`super` escape to the enclosing function scope.

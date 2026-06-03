# DeepWiki Response

Repository: `boa-dev/boa`
Question: For ECMAScript arrow functions, explain the reference behavior and implementation points for parameter environments, TDZ and default parameter self-reference, separate function/parameter/body var environments, lexical super calls from arrow functions created inside constructors, non-strict arrow syntax, and early errors such as duplicate lexical/body names and eval/var scope syntax errors. Please cite relevant implementation files or algorithms.
Search: https://deepwiki.com/search/for-ecmascript-arrow-functions_941874ce-6e66-432e-a2ab-fba6e45bfded

**Question**: For ECMAScript arrow functions, explain the reference behavior and implementation points for parameter environments, TDZ and default parameter self-reference, separate function/parameter/body var environments, lexical super calls from arrow functions created inside constructors, non-strict arrow syntax, and early errors such as duplicate lexical/body names and eval/var scope syntax errors. Please cite relevant implementation files or algorithms.

This query delves into the intricate details of ECMAScript arrow function implementation within the Boa JavaScript engine, focusing on how various lexical and execution environment aspects are handled. The core of the implementation for these features resides within the `boa_ast` crate, specifically in the `scope_analyzer.rs` file, which is responsible for static scope analysis, and the `boa_parser` crate, which handles the syntactic parsing of arrow functions. The `boa_engine` crate, particularly `bytecompiler/declarations.rs`, then uses this analyzed information for bytecode generation.

### Parameter Environments and TDZ

In Boa, the `BindingCollectorVisitor` and `BindingEscapeAnalyzer` in `core/ast/src/scope_analyzer.rs` are central to managing parameter environments [1](#ref-q1-1) . The `visit_function_like` method within `BindingCollectorVisitor` is invoked for arrow functions [2](#ref-q1-2) . This method sets up distinct scopes for parameters and the function body. Specifically, `function_declaration_instantiation` is called to create `FunctionScopes` which encapsulate the `function_scope`, `parameters_eval_scope`, and `lexical_scope` [3](#ref-q1-3) .

The concept of a Temporal Dead Zone (TDZ) for default parameters and self-reference is implicitly handled by the order of binding creation and access within these scopes. When `has_parameter_expressions` is true (indicating default parameters or other expressions in the parameter list), a separate `parameters_eval_scope` is created [4](#ref-q1-4) . This ensures that bindings created by direct `eval` calls in the formal parameter list are isolated from the environment where parameters are declared, which is crucial for correct TDZ behavior.

### Separate Function/Parameter/Body Var Environments

Boa explicitly creates separate environments for function parameters and the function body, especially when parameter expressions are present [5](#ref-q1-5) . The `function_declaration_instantiation` algorithm, implemented in `core/ast/src/scope_analyzer.rs`, details this process [6](#ref-q1-6) .

*   **Function Scope**: The `function_scope` is the outermost scope for the function [7](#ref-q1-7) .
*   **Parameters Eval Scope**: If `has_parameter_expressions` is true, a `parameters_eval_scope` is created as a child of the `function_scope` [4](#ref-q1-4) . This scope is used for evaluating parameter expressions.
*   **Parameters Scope**: The `parameters_scope` is where the actual parameter bindings are created [8](#ref-q1-8) .
*   **Body Scope**: The `body_scope` is where the declarations within the function body are processed [9](#ref-q1-9) .

The `BindingCollectorVisitor` manages the swapping of the current scope (`self.scope`) to correctly populate these distinct environments during AST traversal [10](#ref-q1-10) .

### Lexical Super Calls from Arrow Functions

Arrow functions do not have their own `this` binding or `super` binding. Instead, they lexically inherit these from their enclosing scope. In Boa, this is reflected in the `BindingCollectorVisitor` where `in_arrow` is a flag indicating if the current context is an arrow function [11](#ref-q1-11) . When `visit_this_mut` is called within an arrow function, it explicitly calls `self.scope.escape_this_in_enclosing_function_scope()` [12](#ref-q1-12) . This ensures that `this` is correctly captured from the parent scope.

For `super` calls, the `ContainsSymbol::SuperCall` and `ContainsSymbol::SuperProperty` checks in `core/ast/src/operations/mod.rs` are relevant [13](#ref-q1-13) . Arrow functions are designed to not contain their own `super` bindings. If a `super` call or property access is found within an arrow function, it would refer to the `super` of the enclosing non-arrow function or method. The `visit_arrow_function` and `visit_async_arrow_function` methods in `ContainsSymbolVisitor` explicitly check for `SuperProperty` and `SuperCall` [14](#ref-q1-14) .

### Non-Strict Arrow Syntax

The parsing of arrow functions is handled in `core/parser/src/parser/expression/assignment/mod.rs` [15](#ref-q1-15) . The `ArrowFunction::new` parser is invoked when an arrow function is detected [15](#ref-q1-15) . The `strict` flag is passed down during the scope analysis phase [16](#ref-q1-16) . The `body.strict()` method is used to determine if the function body is in strict mode [17](#ref-q1-17) .

### Early Errors

Boa implements several early error checks for arrow functions during parsing and scope analysis:

*   **Duplicate Parameter Names**: It is a `Syntax Error` if an arrow function's parameters have duplicate names [18](#ref-q1-18) . This is checked by `parameters.has_duplicates()`.
*   **`YieldExpression` / `AwaitExpression` in Parameters**: `YieldExpression` and `AwaitExpression` are not allowed in arrow function parameters [19](#ref-q1-19) . These are checked using `contains(&parameters, ContainsSymbol::YieldExpression)` and `contains(&parameters, ContainsSymbol::AwaitExpression)`.
*   **Strict Mode and Non-Simple Parameter List**: A `Syntax Error` is thrown if a strict mode arrow function has a non-simple parameter list [20](#ref-q1-20) . This is checked by `body.strict() && !parameters.is_simple()`.
*   **Duplicate Lexical/Body Names**: It is a `Syntax Error` if any bound name of the arrow function's parameters also occurs in the lexically declared names of its concise body [21](#ref-q1-21) . This is handled by the `name_in_lexically_declared_names` function.
*   **`eval` / `arguments` in Strict Mode Parameters**: In strict mode, using `eval` or `arguments` as parameter names is a `Syntax Error` [22](#ref-q1-22) . This is checked in `function_declaration_instantiation` within `core/engine/src/builtins/function/mod.rs`.
*   **`super` Usage**: Invalid `super` usage (e.g., `SuperProperty` or `SuperCall`) within a function body or parameters can also lead to `Syntax Error` [23](#ref-q1-23) .

## Notes

The provided context primarily focuses on the static analysis and parsing phases of the Boa engine. While the query touches upon runtime behavior (like TDZ), the implementation details provided are mostly about how these rules are enforced during the compilation of the Abstract Syntax Tree (AST) and the subsequent bytecode generation. The `function_declaration_instantiation` algorithm is a key component that bridges the gap between the AST representation and the runtime environment setup.

Wiki pages you might want to explore:
- [Virtual Machine (boa-dev/boa)](/wiki/boa-dev/boa#2.2)
- [Context & Execution Environment (boa-dev/boa)](/wiki/boa-dev/boa#2.3)
- [Parser & AST (boa-dev/boa)](/wiki/boa-dev/boa#5.4)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-arrow-functions_941874ce-6e66-432e-a2ab-fba6e45bfded

## References

<a id="ref-q1-1"></a>
### [1] `core/ast/src/scope_analyzer.rs:577-583`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L577-L583)

```rust
struct BindingCollectorVisitor<'interner> {
    strict: bool,
    eval: bool,
    scope: Scope,
    in_arrow: bool,
    interner: &'interner Interner,
}
```

<a id="ref-q1-2"></a>
### [2] `core/ast/src/scope_analyzer.rs:747-760`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L747-L760)

```rust
    fn visit_arrow_function_mut(
        &mut self,
        node: &'ast mut ArrowFunction,
    ) -> ControlFlow<Self::BreakTy> {
        let strict = node.body.strict();
        self.visit_function_like(
            &mut node.body,
            &mut node.parameters,
            &mut node.scopes,
            None,
            &mut None,
            strict,
            true,
        )
```

<a id="ref-q1-3"></a>
### [3] `core/ast/src/scope_analyzer.rs:1877-1892`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1877-L1892)

```rust
fn function_declaration_instantiation(
    body: &FunctionBody,
    formals: &FormalParameterList,
    arrow: bool,
    strict: bool,
    function_scope: Scope,
    interner: &Interner,
) -> FunctionScopes {
    let mut scopes = FunctionScopes {
        function_scope,
        parameters_eval_scope: None,
        parameters_scope: None,
        lexical_scope: None,
        mapped_arguments_object: false,
        requires_function_scope: false,
    };
```

<a id="ref-q1-4"></a>
### [4] `core/ast/src/scope_analyzer.rs:1975-1985`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1975-L1985)

```rust
    else {
        // a. NOTE: A separate Environment Record is needed to ensure that bindings created by
        //    direct eval calls in the formal parameter list are outside the environment where parameters are declared.
        // b. Let calleeEnv be the LexicalEnvironment of calleeContext.
        // c. Let env be NewDeclarativeEnvironment(calleeEnv).
        // d. Assert: The VariableEnvironment of calleeContext is calleeEnv.
        // e. Set the LexicalEnvironment of calleeContext to env.
        let scope = Scope::new(scopes.function_scope.clone(), false);
        scopes.parameters_eval_scope = Some(scope.clone());
        scope
    };
```

<a id="ref-q1-5"></a>
### [5] `core/ast/src/scope_analyzer.rs:1968-1985`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1968-L1985)

```rust
    let env = if strict || !has_parameter_expressions {
        // a. NOTE: Only a single Environment Record is needed for the parameters,
        //    since calls to eval in strict mode code cannot create new bindings which are visible outside of the eval.
        // b. Let env be the LexicalEnvironment of calleeContext.
        scopes.function_scope.clone()
    }
    // 20. Else,
    else {
        // a. NOTE: A separate Environment Record is needed to ensure that bindings created by
        //    direct eval calls in the formal parameter list are outside the environment where parameters are declared.
        // b. Let calleeEnv be the LexicalEnvironment of calleeContext.
        // c. Let env be NewDeclarativeEnvironment(calleeEnv).
        // d. Assert: The VariableEnvironment of calleeContext is calleeEnv.
        // e. Set the LexicalEnvironment of calleeContext to env.
        let scope = Scope::new(scopes.function_scope.clone(), false);
        scopes.parameters_eval_scope = Some(scope.clone());
        scope
    };
```

<a id="ref-q1-6"></a>
### [6] `core/ast/src/scope_analyzer.rs:1877-1884`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1877-L1884)

```rust
fn function_declaration_instantiation(
    body: &FunctionBody,
    formals: &FormalParameterList,
    arrow: bool,
    strict: bool,
    function_scope: Scope,
    interner: &Interner,
) -> FunctionScopes {
```

<a id="ref-q1-7"></a>
### [7] `core/ast/src/scope.rs:724-725`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope.rs#L724-L725)

```rust
    pub fn function_scope(&self) -> &Scope {
        &self.function_scope
```

<a id="ref-q1-8"></a>
### [8] `core/ast/src/scope.rs:771-773`
Source: `boa-dev`

<a id="ref-q1-9"></a>
### [9] `core/ast/src/scope_analyzer.rs:1206`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1206)

```rust
        let mut body_scope = function_scopes.body_scope();
```

<a id="ref-q1-10"></a>
### [10] `core/ast/src/scope_analyzer.rs:1208-1214`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1208-L1214)

```rust
        std::mem::swap(&mut self.scope, &mut params_scope);
        self.visit_formal_parameter_list_mut(parameters)?;
        std::mem::swap(&mut self.scope, &mut params_scope);

        std::mem::swap(&mut self.scope, &mut body_scope);
        self.visit_function_body_mut(body)?;
        std::mem::swap(&mut self.scope, &mut body_scope);
```

<a id="ref-q1-11"></a>
### [11] `core/ast/src/scope_analyzer.rs:581`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L581)

```rust
    in_arrow: bool,
```

<a id="ref-q1-12"></a>
### [12] `core/ast/src/scope_analyzer.rs:592-594`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L592-L594)

```rust
        // NOTE: Arrow functions inherit 'this' from their enclosing scope, so we must escape it.
        if self.in_arrow {
            self.scope.escape_this_in_enclosing_function_scope();
```

<a id="ref-q1-13"></a>
### [13] `core/ast/src/operations/mod.rs:265-266`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/operations/mod.rs#L265-L266)

```rust
                ContainsSymbol::SuperProperty,
                ContainsSymbol::SuperCall,
```

<a id="ref-q1-14"></a>
### [14] `core/ast/src/operations/mod.rs:285-286`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/operations/mod.rs#L285-L286)

```rust
                ContainsSymbol::SuperProperty,
                ContainsSymbol::SuperCall,
```

<a id="ref-q1-15"></a>
### [15] `core/parser/src/parser/expression/assignment/mod.rs:114-116`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L114-L116)

```rust
                    return ArrowFunction::new(self.allow_in, self.allow_yield, self.allow_await)
                        .parse(cursor, interner)
                        .map(Expression::ArrowFunction);
```

<a id="ref-q1-16"></a>
### [16] `core/ast/src/scope_analyzer.rs:1179-1180`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L1179-L1180)

```rust
        strict: bool,
        arrow: bool,
```

<a id="ref-q1-17"></a>
### [17] `core/parser/src/parser/expression/assignment/mod.rs:201`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L201)

```rust
                // Early Error: It is a Syntax Error if ConciseBodyContainsUseStrict of ConciseBody is true
```

<a id="ref-q1-18"></a>
### [18] `core/parser/src/parser/expression/assignment/mod.rs:177-183`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L177-L183)

```rust
                // Early Error: ArrowFormalParameters are UniqueFormalParameters.
                if parameters.has_duplicates() {
                    return Err(Error::lex(LexError::Syntax(
                        "Duplicate parameter name not allowed in this context".into(),
                        position,
                    )));
                }
```

<a id="ref-q1-19"></a>
### [19] `core/parser/src/parser/expression/assignment/mod.rs:185-199`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L185-L199)

```rust
                // Early Error: It is a Syntax Error if ArrowParameters Contains YieldExpression is true.
                if contains(&parameters, ContainsSymbol::YieldExpression) {
                    return Err(Error::lex(LexError::Syntax(
                        "Yield expression not allowed in this context".into(),
                        position,
                    )));
                }

                // Early Error: It is a Syntax Error if ArrowParameters Contains AwaitExpression is true.
                if contains(&parameters, ContainsSymbol::AwaitExpression) {
                    return Err(Error::lex(LexError::Syntax(
                        "Await expression not allowed in this context".into(),
                        position,
                    )));
                }
```

<a id="ref-q1-20"></a>
### [20] `core/parser/src/parser/expression/assignment/mod.rs:201-209`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L201-L209)

```rust
                // Early Error: It is a Syntax Error if ConciseBodyContainsUseStrict of ConciseBody is true
                // and IsSimpleParameterList of ArrowParameters is false.
                if body.strict() && !parameters.is_simple() {
                    return Err(Error::lex(LexError::Syntax(
                        "Illegal 'use strict' directive in function with non-simple parameter list"
                            .into(),
                        position,
                    )));
                }
```

<a id="ref-q1-21"></a>
### [21] `core/parser/src/parser/expression/assignment/mod.rs:211-219`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/expression/assignment/mod.rs#L211-L219)

```rust
                // It is a Syntax Error if any element of the BoundNames of ArrowParameters
                // also occurs in the LexicallyDeclaredNames of ConciseBody.
                // https://tc39.es/ecma262/#sec-arrow-function-definitions-static-semantics-early-errors
                name_in_lexically_declared_names(
                    &bound_names(&parameters),
                    &lexically_declared_names(&body),
                    position,
                    interner,
                )?;
```

<a id="ref-q1-22"></a>
### [22] `core/engine/src/builtins/function/mod.rs:576-582`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L576-L582)

```rust
            if body.strict() {
                for name in bound_names(&parameters) {
                    if name == Sym::ARGUMENTS || name == Sym::EVAL {
                        return Err(JsNativeError::syntax()
                            .with_message("Unexpected 'eval' or 'arguments' in strict mode")
                            .into());
                    }
```

<a id="ref-q1-23"></a>
### [23] `core/parser/src/parser/statement/declaration/hoistable/mod.rs:238-247`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/parser/src/parser/statement/declaration/hoistable/mod.rs#L238-L247)

```rust
    // It is a Syntax Error if FormalParameters Contains SuperProperty is true.
    // It is a Syntax Error if FunctionBody Contains SuperProperty is true.
    // It is a Syntax Error if FormalParameters Contains SuperCall is true.
    // It is a Syntax Error if FunctionBody Contains SuperCall is true.
    if contains(&body, ContainsSymbol::Super) || contains(&params, ContainsSymbol::Super) {
        return Err(Error::lex(LexError::Syntax(
            "invalid super usage".into(),
            params_start_position,
        )));
    }
```

# DeepWiki Response

Repository: `engine262/engine262`
Question: For ECMAScript arrow functions, explain the reference behavior and implementation points for parameter environments, TDZ and default parameter self-reference, separate function/parameter/body var environments, lexical super calls from arrow functions created inside constructors, non-strict arrow syntax, and early errors such as duplicate lexical/body names and eval/var scope syntax errors. Please cite relevant implementation files or algorithms.
Search: https://deepwiki.com/search/for-ecmascript-arrow-functions_6c9e9c89-702c-4151-b64a-db470362a09d

**Question**: For ECMAScript arrow functions, explain the reference behavior and implementation points for parameter environments, TDZ and default parameter self-reference, separate function/parameter/body var environments, lexical super calls from arrow functions created inside constructors, non-strict arrow syntax, and early errors such as duplicate lexical/body names and eval/var scope syntax errors. Please cite relevant implementation files or algorithms.

Arrow functions in engine262 are implemented with specific reference behaviors for their lexical environment, `this` binding, and `super` calls, differing from traditional functions [1](#ref-q1-1) . The parsing and instantiation of arrow functions, including asynchronous ones, are handled by dedicated runtime semantics [2](#ref-q1-2)  [3](#ref-q1-3) . The engine also defines how parameter environments are set up, how early errors are managed, and the absence of an `arguments` object for arrow functions [4](#ref-q1-4) .

## Parameter Environments and TDZ

For arrow functions, the lexical environment of the running execution context at the time of their creation is captured as their `[[Environment]]` internal slot [5](#ref-q1-5)  [6](#ref-q1-6) . This means that arrow functions do not create their own lexical environment for parameters in the same way that traditional functions do [7](#ref-q1-7) .

The `FunctionDeclarationInstantiation` algorithm, which sets up the environment for function calls, explicitly checks if a function's `[[ThisMode]]` is `lexical` [8](#ref-q1-8) . If it is, which is the case for arrow functions, it sets `argumentsObjectNeeded` to `false`, indicating that arrow functions do not have an `arguments` object [9](#ref-q1-9) .

The concept of a Temporal Dead Zone (TDZ) primarily applies to `let` and `const` declarations within a lexical environment. While the prompt mentions TDZ in the context of default parameter self-reference, the provided code snippets do not directly illustrate the TDZ mechanism for default parameters in arrow functions. However, the `FunctionDeclarationInstantiation` algorithm does handle parameter binding initialization [10](#ref-q1-10) , which would implicitly involve TDZ for `let`/`const` bound parameters if they were to self-reference before initialization.

## Separate Function/Parameter/Body Var Environments

The `FunctionDeclarationInstantiation` algorithm details how different environments are established for function parameters and the function body [11](#ref-q1-11) .

*   **Parameter Environment**: If a function is not strict and has parameter expressions (e.g., default parameters), a separate declarative environment record (`env`) is created for the parameters, nested within the caller's lexical environment [12](#ref-q1-12) . This `env` is then used to create mutable bindings for each parameter name [13](#ref-q1-13) .
*   **Var Environment**: If `hasParameterExpressions` is `false` (e.g., for simple parameter lists), the `varEnv` is the same as the `env` used for parameters [14](#ref-q1-14) . However, if `hasParameterExpressions` is `true`, a new declarative environment record (`varEnv`) is created, nested within the parameter `env`, to prevent closures in the parameter list from accessing declarations in the function body [15](#ref-q1-15) . This `varEnv` is then set as the `VariableEnvironment` of the execution context [16](#ref-q1-16) .
*   **Lexical Environment**: For non-strict functions, a further declarative environment record (`lexEnv`) is created, nested within `varEnv`, to handle top-level lexical declarations [17](#ref-q1-17) . This `lexEnv` becomes the `LexicalEnvironment` of the execution context [18](#ref-q1-18) . For strict functions, `lexEnv` is simply `varEnv` [19](#ref-q1-19) .

Arrow functions, having `ThisMode` as `lexical`, do not create their own `arguments` object [4](#ref-q1-4) . Their `[[Environment]]` is the lexical environment of their creation, which is used as their `Scope` during `OrdinaryFunctionCreate` [6](#ref-q1-6) .

## Lexical `super` Calls from Arrow Functions

Arrow functions inherit their `this` and `super` bindings from their lexical environment [20](#ref-q1-20) . When an arrow function is created, its `[[ThisMode]]` internal slot is set to `lexical` [1](#ref-q1-1) . This means that `this` and `super` are resolved from the enclosing lexical scope, not from the arrow function's own execution context [21](#ref-q1-21)  [22](#ref-q1-22) .

The `OrdinaryFunctionCreate` abstract operation, used for instantiating arrow functions, sets the `[[ThisMode]]` to `lexical` if the `thisMode` argument is `'lexical-this'` [1](#ref-q1-1) . The `InstantiateArrowFunctionExpression` and `InstantiateAsyncArrowFunctionExpression` runtime semantics both pass `'lexical-this'` to `OrdinaryFunctionCreate` [23](#ref-q1-23)  [24](#ref-q1-24) .

The `FunctionEnvironmentRecord`'s `HasThisBinding` and `HasSuperBinding` methods return `false` if `[[ThisBindingStatus]]` is `lexical` [21](#ref-q1-21)  [22](#ref-q1-22) . This confirms that arrow functions do not establish their own `this` or `super` bindings. Therefore, if an arrow function is created inside a constructor, any `super` calls within the arrow function will lexically bind to the `super` binding of the constructor.

## Non-Strict Arrow Syntax

The parsing of arrow functions is handled by `ExpressionParser.mts` [25](#ref-q1-25) . The parser identifies arrow functions based on the `=>` token [26](#ref-q1-26) . The `parseArrowFunction` method is called for both normal and async arrow functions [25](#ref-q1-25)  [27](#ref-q1-27) .

The `FunctionDeclarationInstantiation` algorithm distinguishes between strict and non-strict functions when setting up environments [28](#ref-q1-28) . However, arrow functions themselves are always strict code if their `ConciseBody` is strict mode code [29](#ref-q1-29) . The `[[Strict]]` internal slot of an ECMAScript function object, including arrow functions, determines its strictness [30](#ref-q1-30) .

## Early Errors

The `Scope` class in `src/parser/Scope.mts` is responsible for tracking declarations and raising early errors during parsing [31](#ref-q1-31) .

*   **Duplicate Lexical/Body Names**: The `declare` method in the `Scope` class checks for duplicate declarations. For `lexical` declarations, it verifies that the name is not already declared as a lexical, variable, function, or parameter in the current scope [32](#ref-q1-32) . If a duplicate is found, it raises an `'AlreadyDeclared'` early error [33](#ref-q1-33) . Similarly, for `function` and `variable` declarations, it checks for conflicts with existing lexical or function declarations [34](#ref-q1-34)  [35](#ref-q1-35) .
*   **`eval`/`var` Scope Syntax Errors**: The `FunctionDeclarationInstantiation` algorithm notes that a separate lexical environment is needed for non-strict functions to determine if `var` scoped declarations introduced by `eval` code conflict with pre-existing top-level lexically scoped declarations [36](#ref-q1-36) . This implies that the parser and runtime are aware of the potential for such conflicts and would raise syntax errors if they occur. The `Scope` class's `declare` method also handles `variable` declarations by iterating through scopes and checking for conflicts with lexical declarations [37](#ref-q1-37) .

## Implementation Points

The core implementation for arrow functions involves:

1.  **Parsing**: The `ExpressionParser` identifies `ArrowFunction` and `AsyncArrowFunction` nodes [38](#ref-q1-38) . The `parseArrowFunction` method is responsible for constructing the AST node for arrow functions, including handling their parameters and concise body [25](#ref-q1-25) .
2.  **Instantiation**: The `InstantiateArrowFunctionExpression` and `InstantiateAsyncArrowFunctionExpression` runtime semantics create the actual function objects [39](#ref-q1-39)  [40](#ref-q1-40) . These operations use `OrdinaryFunctionCreate` to set up the function's internal slots, including `[[Environment]]` (lexical scope), `[[ThisMode]]` (set to `lexical`), and `[[PrivateEnvironment]]` [41](#ref-q1-41)  [42](#ref-q1-42) .
3.  **Evaluation**: The `Evaluate_ArrowFunction` and `Evaluate_AsyncArrowFunction` functions in `src/evaluator.mts` are responsible for evaluating these AST nodes, which primarily involves calling their respective instantiation routines [38](#ref-q1-38) .

## Notes

The provided context offers a strong foundation for understanding arrow function implementation in engine262. While the prompt specifically asked about TDZ and default parameter self-reference, the code snippets primarily focus on the environment setup and early error detection, rather than explicit TDZ checks for default parameters. The `FunctionDeclarationInstantiation` algorithm's handling of `hasParameterExpressions` and the creation of separate environments for parameters and the function body are crucial for managing the scope of default parameter values [12](#ref-q1-12)  [15](#ref-q1-15) . The `IteratorBindingInitialization_FormalParameters` is where the actual binding and initialization of parameters occur [10](#ref-q1-10) , which would be the point where TDZ would manifest if a default parameter tried to reference itself or another parameter in its TDZ.

Wiki pages you might want to explore:
- [Core Architecture (engine262/engine262)](/wiki/engine262/engine262#3)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-arrow-functions_6c9e9c89-702c-4151-b64a-db470362a09d

## References

<a id="ref-q1-1"></a>
### [1] `src/abstract-ops/function-operations.mts:385-387`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L385-L387)

```
  // 10. If thisMode is lexical-this, set F.[[ThisMode]] to lexical.
  if (thisMode === 'lexical-this') {
    F.ThisMode = 'lexical';
```

<a id="ref-q1-2"></a>
### [2] `src/runtime-semantics/InstantiateArrowFunctionExpression.mts:8-9`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateArrowFunctionExpression.mts#L8-L9)

```
// ArrowFunction : ArrowParameters `=>` ConciseBody
export function InstantiateArrowFunctionExpression(ArrowFunction: ParseNode.ArrowFunction, name?: PropertyKeyValue | PrivateName) {
```

<a id="ref-q1-3"></a>
### [3] `src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts:8-9`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts#L8-L9)

```
// AsyncArrowFunction : ArrowParameters `=>` AsyncConciseBody
export function InstantiateAsyncArrowFunctionExpression(AsyncArrowFunction: ParseNode.AsyncArrowFunction, name?: PropertyKeyValue | PrivateName) {
```

<a id="ref-q1-4"></a>
### [4] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:82-86`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L82-L86)

```
  // If func.[[ThisMode]] is lexical, then
  if (func.ThisMode === 'lexical') {
    // a. NOTE: Arrow functions never have an arguments objects.
    // b. Set argumentsObjectNeeded to false.
    argumentsObjectNeeded = false;
```

<a id="ref-q1-5"></a>
### [5] `src/runtime-semantics/InstantiateArrowFunctionExpression.mts:15-16`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateArrowFunctionExpression.mts#L15-L16)

```
  // 2. Let scope be the LexicalEnvironment of the running execution context.
  const scope = surroundingAgent.runningExecutionContext.LexicalEnvironment;
```

<a id="ref-q1-6"></a>
### [6] `src/abstract-ops/function-operations.mts:395-396`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L395-L396)

```
  // 14. Set F.[[Environment]] to Scope.
  F.Environment = Scope;
```

<a id="ref-q1-7"></a>
### [7] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:98-102`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L98-L102)

```
  // 19. If strict is true or if hasParameterExpressions is false, then
  if (strict || hasParameterExpressions === false) {
    // a. NOTE: Only a single lexical environment is needed for the parameters and top-level vars.
    // b. Let env be the LexicalEnvironment of calleeContext.
    env = calleeContext.LexicalEnvironment;
```

<a id="ref-q1-8"></a>
### [8] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:82`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L82)

```
  // If func.[[ThisMode]] is lexical, then
```

<a id="ref-q1-9"></a>
### [9] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:84-86`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L84-L86)

```
    // a. NOTE: Arrow functions never have an arguments objects.
    // b. Set argumentsObjectNeeded to false.
    argumentsObjectNeeded = false;
```

<a id="ref-q1-10"></a>
### [10] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:174-175`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L174-L175)

```
  // Perform ? IteratorBindingInitialization of _formals_ with arguments _iteratorRecord_ and _usedEnv_.
  Q(yield* IteratorBindingInitialization_FormalParameters(formals, iteratorRecord, usedEnv));
```

<a id="ref-q1-11"></a>
### [11] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:29-30`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L29-L30)

```
/** https://tc39.es/ecma262/#sec-functiondeclarationinstantiation */
export function* FunctionDeclarationInstantiation(func: ECMAScriptFunctionObject, argumentsList: Arguments): PlainEvaluator<void> {
```

<a id="ref-q1-12"></a>
### [12] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:103-114`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L103-L114)

```
  } else {
    // a. NOTE: A separate Environment Record is needed to ensure that bindings created by direct eval
    //    calls in the formal parameter list are outside the environment where parameters are declared.
    // b. Let calleeEnv be the LexicalEnvironment of calleeContext.
    const calleeEnv = calleeContext.LexicalEnvironment;
    // c. Let env be NewDeclarativeEnvironment(calleeEnv).
    env = new DeclarativeEnvironmentRecord(calleeEnv);
    // d. Assert: The VariableEnvironment of calleeContext is calleeEnv.
    Assert(calleeContext.VariableEnvironment === calleeEnv);
    // e. Set the LexicalEnvironment of calleeContext to env.
    calleeContext.LexicalEnvironment = env;
  }
```

<a id="ref-q1-13"></a>
### [13] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:123-124`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L123-L124)

```
      // i. Perform ! env.CreateMutableBinding(paramName, false).
      X(env.CreateMutableBinding(paramName, Value.false));
```

<a id="ref-q1-14"></a>
### [14] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:177-195`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L177-L195)

```
  // 27. If hasParameterExpressions is false, then
  if (hasParameterExpressions === false) {
    // a. NOTE: Only a single lexical environment is needed for the parameters and top-level vars.
    // b. Let instantiatedVarNames be a copy of the List parameterBindings.
    const instantiatedVarNames = new JSStringSet(parameterBindings);
    // c. For each n in varNames, do
    for (const n of varNames) {
      // i. If n is not an element of instantiatedVarNames, then
      if (!instantiatedVarNames.has(n)) {
        // 1. Append n to instantiatedVarNames.
        instantiatedVarNames.add(n);
        // 2. Perform ! env.CreateMutableBinding(n, false).
        X(env.CreateMutableBinding(n, Value.false));
        // 3. Call env.InitializeBinding(n, undefined).
        yield* env.InitializeBinding(n, Value.undefined);
      }
    }
    // d. Let varEnv be env.
    varEnv = env;
```

<a id="ref-q1-15"></a>
### [15] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:197-200`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L197-L200)

```
    // a. NOTE: A separate Environment Record is needed to ensure that closures created by expressions
    //    in the formal parameter list do not have visibility of declarations in the function body.
    // b. Let varEnv be NewDeclarativeEnvironment(env).
    varEnv = new DeclarativeEnvironmentRecord(env);
```

<a id="ref-q1-16"></a>
### [16] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:201-202`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L201-L202)

```
    // c. Set the VariableEnvironment of calleeContext to varEnv.
    calleeContext.VariableEnvironment = varEnv;
```

<a id="ref-q1-17"></a>
### [17] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:230-234`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L230-L234)

```
  if (strict === false) {
    // a. Let lexEnv be NewDeclarativeEnvironment(varEnv).
    lexEnv = new DeclarativeEnvironmentRecord(varEnv);
    // b. NOTE: Non-strict functions use a separate lexical Environment Record for top-level lexical declarations
    //    so that a direct eval can determine whether any var scoped declarations introduced by the eval code
```

<a id="ref-q1-18"></a>
### [18] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:241-242`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L241-L242)

```
  // 32. Set the LexicalEnvironment of calleeContext to lexEnv.
  calleeContext.LexicalEnvironment = lexEnv;
```

<a id="ref-q1-19"></a>
### [19] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:238-239`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L238-L239)

```
    // a. Else, let lexEnv be varEnv.
    lexEnv = varEnv;
```

<a id="ref-q1-20"></a>
### [20] `src/abstract-ops/function-operations.mts:156-158`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L156-L158)

```
  const thisMode = F.ThisMode;
  // 2. If thisMode is lexical, return NormalCompletion(undefined).
  if (thisMode === 'lexical') {
```

<a id="ref-q1-21"></a>
### [21] `src/environment.mts:468-470`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L468-L470)

```
    // 2. If envRec.[[ThisBindingStatus]] is lexical, return false; otherwise, return true.
    if (envRec.ThisBindingStatus === 'lexical') {
      return Value.false;
```

<a id="ref-q1-22"></a>
### [22] `src/environment.mts:479-480`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L479-L480)

```
    // 1. If envRec.[[ThisBindingStatus]] is lexical, return false.
    if (envRec.ThisBindingStatus === 'lexical') {
```

<a id="ref-q1-23"></a>
### [23] `src/runtime-semantics/InstantiateArrowFunctionExpression.mts:27`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateArrowFunctionExpression.mts#L27)

```
    'lexical-this',
```

<a id="ref-q1-24"></a>
### [24] `src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts:29`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts#L29)

```
    'lexical-this',
```

<a id="ref-q1-25"></a>
### [25] `src/parser/ExpressionParser.mts:96-97`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ExpressionParser.mts#L96-L97)

```
        return this.parseArrowFunction(node, { Arguments: [left] }, FunctionKind.NORMAL);
      }
```

<a id="ref-q1-26"></a>
### [26] `src/parser/ExpressionParser.mts:93`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ExpressionParser.mts#L93)

```
      if (this.test(Token.ARROW) && !this.peek().hadLineTerminatorBefore) {
```

<a id="ref-q1-27"></a>
### [27] `src/parser/ExpressionParser.mts:107-108`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ExpressionParser.mts#L107-L108)

```
        return this.parseArrowFunction(node, left, FunctionKind.ASYNC);
      }
```

<a id="ref-q1-28"></a>
### [28] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:98`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L98)

```
  // 19. If strict is true or if hasParameterExpressions is false, then
```

<a id="ref-q1-29"></a>
### [29] `src/abstract-ops/function-operations.mts:381-382`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L381-L382)

```
  // 8. If the source text matching Body is strict mode code, let Strict be true; else let Strict be false.
  const Strict = isStrictModeCode(Body);
```

<a id="ref-q1-30"></a>
### [30] `src/abstract-ops/function-operations.mts:362`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L362)

```
    'Strict',
```

<a id="ref-q1-31"></a>
### [31] `src/parser/Scope.mts:172`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L172)

```
export class Scope {
```

<a id="ref-q1-32"></a>
### [32] `src/parser/Scope.mts:416-420`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L416-L420)

```
          const scope = this.lexicalScope();
          if (scope.lexicals.has(d.name)
              || scope.variables.has(d.name)
              || scope.functions.has(d.name)
              || scope.parameters.has(d.name)) {
```

<a id="ref-q1-33"></a>
### [33] `src/parser/Scope.mts:421`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L421)

```
            this.parser.raiseEarly('AlreadyDeclared', d.node, d.name);
```

<a id="ref-q1-34"></a>
### [34] `src/parser/Scope.mts:431-432`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L431-L432)

```
          if (scope.lexicals.has(d.name)) {
            this.parser.raiseEarly('AlreadyDeclared', d.node, d.name);
```

<a id="ref-q1-35"></a>
### [35] `src/parser/Scope.mts:453-455`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L453-L455)

```
            scope.variables.add(d.name);
            if (scope.lexicals.has(d.name) || (!scope.flags.variableFunctions && scope.functions.has(d.name))) {
              this.parser.raiseEarly('AlreadyDeclared', d.node, d.name);
```

<a id="ref-q1-36"></a>
### [36] `src/runtime-semantics/FunctionDeclarationInstantiation.mts:233-236`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/FunctionDeclarationInstantiation.mts#L233-L236)

```
    // b. NOTE: Non-strict functions use a separate lexical Environment Record for top-level lexical declarations
    //    so that a direct eval can determine whether any var scoped declarations introduced by the eval code
    //    conflict with pre-existing top-level lexically scoped declarations. This is not needed for strict functions
    //    because a strict direct eval always places all declarations into a new Environment Record.
```

<a id="ref-q1-37"></a>
### [37] `src/parser/Scope.mts:450-463`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/Scope.mts#L450-L463)

```
        case 'variable':
          for (let i = this.scopeStack.length - 1; i >= 0; i -= 1) {
            const scope = this.scopeStack[i];
            scope.variables.add(d.name);
            if (scope.lexicals.has(d.name) || (!scope.flags.variableFunctions && scope.functions.has(d.name))) {
              this.parser.raiseEarly('AlreadyDeclared', d.node, d.name);
            }
            if (i === 0 && this.undefinedExports.has(d.name)) {
              this.undefinedExports.delete(d.name);
            }
            if (scope.flags.variable) {
              break;
            }
          }
```

<a id="ref-q1-38"></a>
### [38] `src/evaluator.mts:262-265`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/evaluator.mts#L262-L265)

```
    case 'ArrowFunction':
      return Evaluate_ArrowFunction(node);
    case 'AsyncArrowFunction':
      return Evaluate_AsyncArrowFunction(node);
```

<a id="ref-q1-39"></a>
### [39] `src/runtime-semantics/InstantiateArrowFunctionExpression.mts:9`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateArrowFunctionExpression.mts#L9)

```
export function InstantiateArrowFunctionExpression(ArrowFunction: ParseNode.ArrowFunction, name?: PropertyKeyValue | PrivateName) {
```

<a id="ref-q1-40"></a>
### [40] `src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts:9`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateAsyncArrowFunctionExpression.mts#L9)

```
export function InstantiateAsyncArrowFunctionExpression(AsyncArrowFunction: ParseNode.AsyncArrowFunction, name?: PropertyKeyValue | PrivateName) {
```

<a id="ref-q1-41"></a>
### [41] `src/runtime-semantics/InstantiateArrowFunctionExpression.mts:22-30`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/InstantiateArrowFunctionExpression.mts#L22-L30)

```
  const closure = OrdinaryFunctionCreate(
    surroundingAgent.intrinsic('%Function.prototype%'),
    sourceText,
    ArrowParameters,
    ConciseBody,
    'lexical-this',
    scope,
    privateScope,
  );
```

<a id="ref-q1-42"></a>
### [42] `src/abstract-ops/function-operations.mts:349`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L349)

```
export function OrdinaryFunctionCreate(functionPrototype: ObjectValue, sourceText: string, ParameterList: ParseNode.FormalParameters, Body: Body, thisMode: 'lexical-this' | 'non-lexical-this', Scope: EnvironmentRecord, PrivateEnv: PrivateEnvironmentRecord | NullValue) {
```

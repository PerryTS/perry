# DeepWiki Response

Repository: `engine262/engine262`
Question: For ECMAScript AdditionExpression evaluation, explain the implementation flow for evaluating left and right operands, GetValue on references, assignment expression result values like (y = 1), ToPrimitive for addition, ToString for objects/functions, ordinary object default stringification, function default stringification, and observable evaluation order. Please cite the relevant files/functions.
Search: https://deepwiki.com/search/for-ecmascript-additionexpress_edf0fe6c-e8ac-43f8-99b3-0dd43aad2612

**Question**: For ECMAScript AdditionExpression evaluation, explain the implementation flow for evaluating left and right operands, GetValue on references, assignment expression result values like (y = 1), ToPrimitive for addition, ToString for objects/functions, ordinary object default stringification, function default stringification, and observable evaluation order. Please cite the relevant files/functions.

The evaluation of an `AdditiveExpression` in engine262 involves several steps, including the evaluation of its left and right operands, handling of `ReferenceRecord`s via `GetValue`, `ToPrimitive` conversion for addition, and `ToString` conversion for objects and functions. The overall evaluation order is observable and follows the ECMAScript specification.

### Evaluation of Left and Right Operands
The `Evaluate_AdditiveExpression` function is responsible for evaluating `AdditiveExpression` nodes [1](#ref-q1-1) . The parser constructs an `AdditiveExpression` node, which contains `AdditiveExpression` (left operand) and `MultiplicativeExpression` (right operand) properties, along with an `operator` property [2](#ref-q1-2) . The `Evaluate` function dispatches to `Evaluate_AdditiveExpression` when it encounters an `AdditiveExpression` node [1](#ref-q1-1) .

### GetValue on References
During evaluation, if an operand evaluates to a `ReferenceRecord`, the `GetValue` abstract operation is called to retrieve the actual value. For example, in `Evaluate_UnaryExpression_Void`, `GetValue` is explicitly called on the result of evaluating the `UnaryExpression` [3](#ref-q1-3) . Similarly, `Evaluate_UnaryExpression_Typeof` also calls `GetValue` after checking if the result is an unresolvable `ReferenceRecord` [4](#ref-q1-4) .

### Assignment Expression Result Values
The prompt mentions assignment expression result values like `(y = 1)`. While the detailed evaluation of `AssignmentExpression` is not provided in the snippets, the `Evaluate` function does have a case for `AssignmentExpression`, which dispatches to `Evaluate_AssignmentExpression` [5](#ref-q1-5) . Typically, an assignment expression evaluates to the value that was assigned.

### ToPrimitive for Addition
For addition, the `ToPrimitive` abstract operation is crucial. The `ToPrimitive` function takes an `input` value and an optional `preferredType` hint ('string' or 'number') [6](#ref-q1-6) .
1.  If the `input` is an `ObjectValue`, it first checks for a `@@toPrimitive` method [7](#ref-q1-7) .
2.  If `@@toPrimitive` exists, it's called with a hint ('default', 'string', or 'number') [8](#ref-q1-8) . If the result is not an object, it's returned; otherwise, a `TypeError` is thrown [9](#ref-q1-9) .
3.  If `@@toPrimitive` is not defined, `OrdinaryToPrimitive` is called with the `input` and the `preferredType` (defaulting to 'number' if not specified) [10](#ref-q1-10) .
4.  If the `input` is not an `ObjectValue`, the `input` itself is returned [11](#ref-q1-11) .

The `OrdinaryToPrimitive` function attempts to convert an object to a primitive value [12](#ref-q1-12) . It tries calling `toString` then `valueOf` if the hint is 'string', or `valueOf` then `toString` if the hint is 'number' [13](#ref-q1-13) . If a callable method returns a non-object primitive, that value is returned [14](#ref-q1-14) . If no primitive is obtained, a `TypeError` is thrown [15](#ref-q1-15) .

### ToString for Objects/Functions
The `ToString` abstract operation is used to convert a value to a `JSStringValue` [16](#ref-q1-16) . For `ObjectValue`s, `ToString` first calls `ToPrimitive` with a 'string' hint [17](#ref-q1-17) . The result of `ToPrimitive` is then converted to a string [18](#ref-q1-18) .

#### Ordinary Object Default Stringification
For ordinary objects, the `OrdinaryToPrimitive` process (called by `ToPrimitive` with a 'string' hint) will first attempt to call the object's `toString` method, then its `valueOf` method [19](#ref-q1-19) . The default `Object.prototype.toString` typically returns `"[object Object]"`.

#### Function Default Stringification
For functions, `IsCallable` is used to determine if a value is a function [20](#ref-q1-20) . When `ToString` is applied to a function object, its `toString` method is typically called. The default `Function.prototype.toString` usually returns a string representation of the function's source code.

### Observable Evaluation Order
The evaluation order is strictly defined by the ECMAScript specification and implemented in engine262. For `AdditiveExpression`, the left operand is evaluated first, then the right operand [21](#ref-q1-21) . This is observable through the `Evaluate` function's generator-based architecture, which allows for precise control over the order of operations and integration with debugging hooks [22](#ref-q1-22) . The `surroundingAgent.runningExecutionContext.callSite.setLocation(node)` call at the beginning of `Evaluate` also ensures that the current evaluation context is tracked, making the evaluation order observable [23](#ref-q1-23) .

## Notes
The provided snippets offer a good overview of the evaluation process for expressions, type conversions, and the core evaluation engine. The `Core Architecture` wiki page also provides a high-level understanding of how the `Evaluate` function dispatches to specific evaluation functions based on the AST node type [24](#ref-q1-24) .

Wiki pages you might want to explore:
- [Core Architecture (engine262/engine262)](/wiki/engine262/engine262#3)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-additionexpress_edf0fe6c-e8ac-43f8-99b3-0dd43aad2612

## References

<a id="ref-q1-1"></a>
### [1] `src/evaluator.mts:204-205`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/evaluator.mts#L204-L205)

```
    case 'AdditiveExpression':
      return yield* Evaluate_AdditiveExpression(node);
```

<a id="ref-q1-2"></a>
### [2] `src/parser/ParseNode.mts:678-686`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ParseNode.mts#L678-L686)

```
  // AdditiveExpression (partial) :
  //   AdditiveExpression `+` MultiplicativeExpression
  //   AdditiveExpression `-` MultiplicativeExpression
  export interface AdditiveExpression extends BaseParseNode {
    readonly type: 'AdditiveExpression';
    readonly operator: '+' | '-';
    readonly AdditiveExpression: AdditiveExpressionOrHigher;
    readonly MultiplicativeExpression: MultiplicativeExpressionOrHigher;
  }
```

<a id="ref-q1-3"></a>
### [3] `src/runtime-semantics/UnaryExpression.mts:82-85`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/UnaryExpression.mts#L82-L85)

```
  // 1. Let expr be the result of evaluating UnaryExpression.
  const expr = Q(yield* Evaluate(UnaryExpression));
  // 2. Perform ? GetValue(expr).
  Q(yield* GetValue(expr));
```

<a id="ref-q1-4"></a>
### [4] `src/runtime-semantics/UnaryExpression.mts:96-103`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/UnaryExpression.mts#L96-L103)

```
  if (_val instanceof ReferenceRecord) {
    // a. If IsUnresolvableReference(val) is true, return "undefined".
    if (IsUnresolvableReference(_val) === Value.true) {
      return Value('undefined');
    }
  }
  // 3. Set val to ? GetValue(val).
  const val = Q(yield* GetValue(_val));
```

<a id="ref-q1-5"></a>
### [5] `src/evaluator.mts:254-255`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/evaluator.mts#L254-L255)

```
    case 'AssignmentExpression':
      return yield* Evaluate_AssignmentExpression(node);
```

<a id="ref-q1-6"></a>
### [6] `src/abstract-ops/type-conversion.mts:41`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L41)

```
export function* ToPrimitive(input: Value, preferredType?: 'string' | 'number'): ValueEvaluator<PrimitiveValue> {
```

<a id="ref-q1-7"></a>
### [7] `src/abstract-ops/type-conversion.mts:45-47`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L45-L47)

```
  if (input instanceof ObjectValue) {
    // a. Let exoticToPrim be ? GetMethod(input, @@toPrimitive).
    const exoticToPrim = Q(yield* GetMethod(input, wellKnownSymbols.toPrimitive));
```

<a id="ref-q1-8"></a>
### [8] `src/abstract-ops/type-conversion.mts:51-63`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L51-L63)

```
      // i. If preferredType is not present, let hint be "default".
      if (preferredType === undefined) {
        hint = Value('default');
      } else if (preferredType === 'string') { // ii. Else if preferredType is string, let hint be "string".
        hint = Value('string');
      } else { // iii. Else,
        // 1. Assert: preferredType is number.
        Assert(preferredType === 'number');
        // 2. Let hint be "number".
        hint = Value('number');
      }
      // iv. Let result be ? Call(exoticToPrim, input, « hint »).
      const result = Q(yield* Call(exoticToPrim, input, [hint]));
```

<a id="ref-q1-9"></a>
### [9] `src/abstract-ops/type-conversion.mts:64-69`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L64-L69)

```
      // v. If Type(result) is not Object, return result.
      if (!(result instanceof ObjectValue)) {
        return result;
      }
      // vi. Throw a TypeError exception.
      return surroundingAgent.Throw('TypeError', 'ObjectToPrimitive');
```

<a id="ref-q1-10"></a>
### [10] `src/abstract-ops/type-conversion.mts:71-76`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L71-L76)

```
    // c. If preferredType is not present, let preferredType be number.
    if (preferredType === undefined) {
      preferredType = 'number';
    }
    // d. Return ? OrdinaryToPrimitive(input, preferredType).
    return Q(yield* OrdinaryToPrimitive(input, preferredType));
```

<a id="ref-q1-11"></a>
### [11] `src/abstract-ops/type-conversion.mts:78-79`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L78-L79)

```
  // 3. Return input.
  return input;
```

<a id="ref-q1-12"></a>
### [12] `src/abstract-ops/type-conversion.mts:82`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L82)

```
/** https://tc39.es/ecma262/#sec-ordinarytoprimitive */
```

<a id="ref-q1-13"></a>
### [13] `src/abstract-ops/type-conversion.mts:89-96`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L89-L96)

```
  // 3. If hint is string, then
  if (hint === 'string') {
    // a. Let methodNames be « "toString", "valueOf" ».
    methodNames = [Value('toString'), Value('valueOf')];
  } else { // 4. Else,
    // a. Let methodNames be « "valueOf", "toString" ».
    methodNames = [Value('valueOf'), Value('toString')];
  }
```

<a id="ref-q1-14"></a>
### [14] `src/abstract-ops/type-conversion.mts:97-108`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L97-L108)

```
  // 5. For each element name of methodNames, do
  for (const name of methodNames) {
    // a. Let method be ? Get(O, name).
    const method = Q(yield* Get(O, name));
    // b. If IsCallable(method) is true, then
    if (IsCallable(method)) {
      // i. Let result be ? Call(method, O).
      const result = Q(yield* Call(method, O));
      // ii. If Type(result) is not Object, return result.
      if (!(result instanceof ObjectValue)) {
        return result;
      }
```

<a id="ref-q1-15"></a>
### [15] `src/abstract-ops/type-conversion.mts:111-112`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L111-L112)

```
  // 6. Throw a TypeError exception.
  return surroundingAgent.Throw('TypeError', 'ObjectToPrimitive');
```

<a id="ref-q1-16"></a>
### [16] `src/abstract-ops/type-conversion.mts:454-458`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L454-L458)

```
  } else if (argument instanceof ObjectValue) {
    // 1. Let primValue be ? ToPrimitive(argument, string).
    const primValue = Q(yield* ToPrimitive(argument, 'string'));
    // 2. Return ? ToString(primValue).
    return Q(yield* ToString(primValue));
```

<a id="ref-q1-17"></a>
### [17] `src/abstract-ops/type-conversion.mts:454-456`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L454-L456)

```
  } else if (argument instanceof ObjectValue) {
    // 1. Let primValue be ? ToPrimitive(argument, string).
    const primValue = Q(yield* ToPrimitive(argument, 'string'));
```

<a id="ref-q1-18"></a>
### [18] `src/abstract-ops/type-conversion.mts:457-458`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L457-L458)

```
    // 2. Return ? ToString(primValue).
    return Q(yield* ToString(primValue));
```

<a id="ref-q1-19"></a>
### [19] `src/abstract-ops/type-conversion.mts:90-92`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L90-L92)

```
  if (hint === 'string') {
    // a. Let methodNames be « "toString", "valueOf" ».
    methodNames = [Value('toString'), Value('valueOf')];
```

<a id="ref-q1-20"></a>
### [20] `src/abstract-ops/type-conversion.mts:101`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/type-conversion.mts#L101)

```
    // b. If IsCallable(method) is true, then
```

<a id="ref-q1-21"></a>
### [21] `src/parser/ExpressionParser.mts:361-363`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ExpressionParser.mts#L361-L363)

```
              node = this.startNode<ParseNode.AdditiveExpression>(left);
              node.AdditiveExpression = left as ParseNode.AdditiveExpressionOrHigher; // NOTE: unsound cast
              node.MultiplicativeExpression = right as ParseNode.MultiplicativeExpressionOrHigher; // NOTE: unsound cast
```

<a id="ref-q1-22"></a>
### [22] `src/evaluator.mts:98-107`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/evaluator.mts#L98-L107)

```
export function* Evaluate(node: ParseNode): Evaluator<unknown> {
  surroundingAgent.runningExecutionContext.callSite.setLocation(node);

  if (surroundingAgent.hostDefinedOptions.onNodeEvaluation) {
    surroundingAgent.hostDefinedOptions.onNodeEvaluation(node, surroundingAgent.currentRealmRecord);
  }
  if (surroundingAgent.hostDefinedOptions.onDebugger) {
    const resumption = yield { type: 'potential-debugger' };
    Assert(resumption.type === 'debugger-resume');
  }
```

<a id="ref-q1-23"></a>
### [23] `src/evaluator.mts:99`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/evaluator.mts#L99)

```
  surroundingAgent.runningExecutionContext.callSite.setLocation(node);
```

<a id="ref-q1-24"></a>
### [24] `Core Architecture:100`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/Core Architecture#L100)

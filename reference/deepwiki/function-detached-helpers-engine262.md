# DeepWiki Response

Repository: `engine262/engine262`
Question: For Function.prototype.call/apply/bind in engine262, how are bound functions represented and invoked? Please focus on detached helper patterns such as Function.prototype.call.bind(Object.prototype.hasOwnProperty), IsCallable checks, Array.isArray as a callable builtin, and creation of function name/length property descriptors for ordinary and bound functions. Include relevant source paths and citations.
Search: https://deepwiki.com/search/for-functionprototypecallapply_3b4a99f9-2a43-46cc-8323-eaee41799f1d

**Question**: For Function.prototype.call/apply/bind in engine262, how are bound functions represented and invoked? Please focus on detached helper patterns such as Function.prototype.call.bind(Object.prototype.hasOwnProperty), IsCallable checks, Array.isArray as a callable builtin, and creation of function name/length property descriptors for ordinary and bound functions. Include relevant source paths and citations.

In engine262, bound functions are represented as `BoundFunctionObject`s and their invocation is handled by the `BoundFunctionExoticObjectCall` and `BoundFunctionExoticObjectConstruct` internal methods [1](#ref-q1-1)  [2](#ref-q1-2)  [3](#ref-q1-3) . The `Function.prototype.bind` method creates these bound function objects [4](#ref-q1-4) , while `Function.prototype.call` and `Function.prototype.apply` are used to invoke callable functions with a specified `this` value and arguments [5](#ref-q1-5)  [6](#ref-q1-6) .

## Bound Function Representation and Creation

A bound function is represented by the `BoundFunctionObject` interface, which extends `ExoticObject` and `BuiltinFunctionObject` [1](#ref-q1-1) . It contains internal slots:
*   `BoundTargetFunction`: The original function that was bound [7](#ref-q1-7) .
*   `BoundThis`: The `this` value to be used when the bound function is called [8](#ref-q1-8) .
*   `BoundArguments`: A list of arguments to be prepended to any arguments passed during invocation [9](#ref-q1-9) .

The `FunctionProto_bind` method, which implements `Function.prototype.bind`, is responsible for creating a `BoundFunctionObject` [4](#ref-q1-4) . It first checks if the `Target` (the `this` value of the `bind` call) is callable using `IsCallable` [10](#ref-q1-10) . If it is, `BoundFunctionCreate` is called to construct the bound function object [11](#ref-q1-11) .

The `BoundFunctionCreate` abstract operation performs the following steps:
1.  It creates a basic object with specific internal slots, including `BoundTargetFunction`, `BoundThis`, and `BoundArguments` [12](#ref-q1-12) .
2.  It sets the `[[Call]]` internal method of the new object to `BoundFunctionExoticObjectCall` [13](#ref-q1-13) .
3.  If the `targetFunction` is a constructor, it also sets the `[[Construct]]` internal method to `BoundFunctionExoticObjectConstruct` [14](#ref-q1-14) .
4.  It populates the `BoundTargetFunction`, `BoundThis`, and `BoundArguments` internal slots with the provided values [15](#ref-q1-15) .

## Invocation of Bound Functions

When a `BoundFunctionObject` is invoked, its `[[Call]]` internal method, `BoundFunctionExoticObjectCall`, is executed [2](#ref-q1-2) . This method retrieves the `BoundTargetFunction`, `BoundThis`, and `BoundArguments` from the bound function object [16](#ref-q1-16) . It then concatenates the `BoundArguments` with the `argumentsList` provided during the current invocation and calls the `BoundTargetFunction` using the `Call` abstract operation, passing `BoundThis` as the `this` value and the combined arguments [17](#ref-q1-17) .

Similarly, if a `BoundFunctionObject` is used with the `new` operator, its `[[Construct]]` internal method, `BoundFunctionExoticObjectConstruct`, is invoked [3](#ref-q1-3) . This method asserts that the `BoundTargetFunction` is a constructor [18](#ref-q1-18) . It combines the `BoundArguments` with the `argumentsList` and then calls the `Construct` abstract operation on the `BoundTargetFunction` [19](#ref-q1-19) .

## `Function.prototype.call` and `Function.prototype.apply`

The `FunctionProto_call` method implements `Function.prototype.call` [5](#ref-q1-5) . It takes a `thisArg` and a variable number of `args` [20](#ref-q1-20) . It first checks if the `func` (the `this` value of the `call` call) is callable using `IsCallable` [21](#ref-q1-21) . It then constructs an `argList` from the provided `args` and invokes `Call(func, thisArg, argList)` [22](#ref-q1-22) .

The `FunctionProto_apply` method implements `Function.prototype.apply` [6](#ref-q1-6) . Similar to `call`, it checks if the `func` is callable [23](#ref-q1-23) . If `argArray` is `undefined` or `null`, it calls `Call(func, thisArg)` [24](#ref-q1-24) . Otherwise, it uses `CreateListFromArrayLike` to convert `argArray` into a list of arguments and then calls `Call(func, thisArg, argList)` [25](#ref-q1-25) .

## `IsCallable` Checks

The `IsCallable` abstract operation determines if a `Value` is a callable function [26](#ref-q1-26) . It returns `true` if the argument is an `ObjectValue` and has a `Call` internal method [27](#ref-q1-27) . This check is crucial for `Function.prototype.call`, `apply`, and `bind` to ensure that the target is indeed a function that can be invoked [23](#ref-q1-23)  [10](#ref-q1-10)  [21](#ref-q1-21) .

## Function Name and Length Property Descriptors

For both ordinary and bound functions, the `name` and `length` properties are set as non-writable, non-enumerable, and configurable data properties [28](#ref-q1-28)  [29](#ref-q1-29) .

The `SetFunctionName` abstract operation sets the `name` property of a function [30](#ref-q1-30) . For bound functions, the `FunctionProto_bind` method calls `SetFunctionName` with a `prefix` of `"bound"` [31](#ref-q1-31) . This results in a name like "bound funcName" for bound functions [32](#ref-q1-32) .

The `SetFunctionLength` abstract operation sets the `length` property of a function [33](#ref-q1-33) . For bound functions, the `FunctionProto_bind` method calculates the `length` based on the `target` function's length and the number of `BoundArguments` [34](#ref-q1-34) . The calculated length is then passed to `SetFunctionLength` [35](#ref-q1-35) .

## Detached Helper Patterns

The pattern `Function.prototype.call.bind(Object.prototype.hasOwnProperty)` is a way to create a "detached" version of `hasOwnProperty` that can be called with any object as its `this` value.
1.  `Function.prototype.call` is a built-in function [36](#ref-q1-36) .
2.  Calling `.bind(Object.prototype.hasOwnProperty)` on `Function.prototype.call` creates a new bound function [4](#ref-q1-4) .
3.  In this bound function, `Function.prototype.call` becomes the `BoundTargetFunction`, and `Object.prototype.hasOwnProperty` becomes the `BoundThis` [37](#ref-q1-37) .
4.  When this bound function is invoked, `BoundFunctionExoticObjectCall` will effectively call `Function.prototype.call.call(Object.prototype.hasOwnProperty, ...)` [2](#ref-q1-2) . This means `Object.prototype.hasOwnProperty` will be invoked with the `this` value and arguments provided to the bound function.

## `Array.isArray` as a Callable Builtin

`Array.isArray` is a built-in function [38](#ref-q1-38) . Built-in functions are created using `CreateBuiltinFunction` [39](#ref-q1-39) . They have a `Call` internal method set to `BuiltinFunctionCall` [40](#ref-q1-40) , which makes them callable according to `IsCallable` [26](#ref-q1-26) . The `BuiltinFunctionCall` method executes the native steps associated with the built-in function [41](#ref-q1-41)  [42](#ref-q1-42) .

## Notes
The provided context focuses on the core mechanisms of `Function.prototype.call`, `apply`, and `bind` within engine262. It details how bound functions are structured and how their invocation is delegated to the target function with the pre-bound `this` and arguments. The `IsCallable` abstract operation is fundamental to these operations, ensuring that only valid functions are processed. The setting of `name` and `length` properties for bound functions also follows specific rules to reflect their bound nature.

View this search on DeepWiki: https://deepwiki.com/search/for-functionprototypecallapply_3b4a99f9-2a43-46cc-8323-eaee41799f1d

## References

<a id="ref-q1-1"></a>
### [1] `src/intrinsics/FunctionPrototype.mts:45-49`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L45-L49)

```
export interface BoundFunctionObject extends ExoticObject, BuiltinFunctionObject {
  readonly BoundTargetFunction: FunctionObject;
  readonly BoundThis: Value;
  readonly BoundArguments: Arguments;
}
```

<a id="ref-q1-2"></a>
### [2] `src/intrinsics/FunctionPrototype.mts:83-92`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L83-L92)

```

function* BoundFunctionExoticObjectCall(this: BoundFunctionObject, _thisArgument: ObjectValue, argumentsList: Arguments): ValueEvaluator {
  const F = this;

  const target = F.BoundTargetFunction;
  const boundThis = F.BoundThis;
  const boundArgs = F.BoundArguments;
  const args = [...boundArgs, ...argumentsList];
  return Q(yield* Call(target, boundThis, args));
}
```

<a id="ref-q1-3"></a>
### [3] `src/intrinsics/FunctionPrototype.mts:94-105`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L94-L105)

```
function* BoundFunctionExoticObjectConstruct(this: BoundFunctionObject, argumentsList: Arguments, newTarget: FunctionObject | UndefinedValue): ValueEvaluator<ObjectValue> {
  const F = this;

  const target = F.BoundTargetFunction;
  Assert(IsConstructor(target));
  const boundArgs = F.BoundArguments;
  const args = [...boundArgs, ...argumentsList];
  if (SameValue(F, newTarget) === Value.true) {
    newTarget = target;
  }
  return Q(yield* Construct(target, args, newTarget));
}
```

<a id="ref-q1-4"></a>
### [4] `src/intrinsics/FunctionPrototype.mts:142-192`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L142-L192)

```
/** https://tc39.es/ecma262/#sec-function.prototype.bind */
function* FunctionProto_bind([thisArg = Value.undefined, ...args]: Arguments, { thisValue }: FunctionCallContext): ValueEvaluator {
  // 1. Let Target be the this value.
  const Target = thisValue;
  // 2. If IsCallable(Target) is false, throw a TypeError exception.
  if (!IsCallable(Target)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', Target);
  }
  __ts_cast__<ObjectValue>(Target);
  // 3. Let F be ? BoundFunctionCreate(Target, thisArg, args).
  const F = Q(yield* BoundFunctionCreate(Target, thisArg, args));
  // 4. Let L be 0.
  let L = 0;
  // 5. Let targetHasLength be ? HasOwnProperty(Target, "length").
  const targetHasLength = Q(yield* HasOwnProperty(Target, Value('length')));
  // 6. If targetHasLength is true, then
  if (targetHasLength === Value.true) {
    // a. Let targetLen be ? Get(Target, "length").
    const targetLen = Q(yield* Get(Target, Value('length')));
    // b. If Type(targetLen) is Number, then
    if (targetLen instanceof NumberValue) {
      // i. If targetLen is +∞𝔽, set L to +∞.
      if (R(targetLen) === +Infinity) {
        L = +Infinity;
      } else if (R(targetLen) === -Infinity) { // ii. Else if targetLen is -∞𝔽, set L to 0.
        L = 0;
      } else { // iii. Else,
        // 1. Set targetLen to ! ToIntegerOrInfinity(targetLen).
        const targetLenAsInt = Q(yield* ToIntegerOrInfinity(targetLen));
        // 2. Assert: targetLenAsInt is finite.
        Assert(Number.isFinite(targetLenAsInt));
        // 3. Let argCount be the number of elements in args.
        const argCount = args.length;
        // 4. Set L to max(targetLenAsInt - argCount, 0).
        L = Math.max(targetLenAsInt - argCount, 0);
      }
    }
  }
  // 7. Perform ! SetFunctionLength(F, L).
  X(SetFunctionLength(F, L));
  // 8. Let targetName be ? Get(Target, "name").
  let targetName = Q(yield* Get(Target, Value('name')));
  // 9. If Type(targetName) is not String, set targetName to the empty String.
  if (!(targetName instanceof JSStringValue)) {
    targetName = Value('');
  }
  // 10. Perform SetFunctionName(F, targetName, "bound").
  SetFunctionName(F, targetName, Value('bound'));
  // 11. Return F.
  return F;
}
```

<a id="ref-q1-5"></a>
### [5] `src/intrinsics/FunctionPrototype.mts:194-212`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L194-L212)

```
/** https://tc39.es/ecma262/#sec-function.prototype.call */
function* FunctionProto_call([thisArg = Value.undefined, ...args]: Arguments, { thisValue }: FunctionCallContext): ValueEvaluator {
  // 1. Let func be the this value.
  const func = thisValue;
  // 2. If IsCallable(func) is false, throw a TypeError exception.
  if (!IsCallable(func)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', func);
  }
  // 3. Let argList be a new empty List.
  const argList = [];
  // 4. If this method was called with more than one argument, then in left to right order, starting with the second argument, append each argument as the last element of argList.
  for (const arg of args) {
    argList.push(arg);
  }
  // 5. Perform PrepareForTailCall().
  PrepareForTailCall();
  // 6. Return ? Call(func, thisArg, argList).
  return Q(yield* Call(func, thisArg, argList));
}
```

<a id="ref-q1-6"></a>
### [6] `src/intrinsics/FunctionPrototype.mts:61-82`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L61-L82)

```
/** https://tc39.es/ecma262/#sec-function.prototype.apply */
function* FunctionProto_apply([thisArg = Value.undefined, argArray = Value.undefined]: Arguments, { thisValue }: FunctionCallContext): ValueEvaluator {
  // 1. Let func be the this value.
  const func = thisValue;
  // 2. If IsCallable(func) is false, throw a TypeError exception.
  if (!IsCallable(func)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', func);
  }
  // 3. If argArray is undefined or null, then
  if (argArray === Value.undefined || argArray === Value.null) {
    // a. Perform PrepareForTailCall().
    PrepareForTailCall();
    // b. Return ? Call(func, thisArg).
    return Q(yield* Call(func, thisArg));
  }
  // 4. Let argList be ? CreateListFromArrayLike(argArray).
  const argList = Q(yield* CreateListFromArrayLike(argArray));
  // 5. Perform PrepareForTailCall().
  PrepareForTailCall();
  // 6. Return ? Call(func, thisArg, argList).
  return Q(yield* Call(func, thisArg, argList));
}
```

<a id="ref-q1-7"></a>
### [7] `src/intrinsics/FunctionPrototype.mts:46`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L46)

```
  readonly BoundTargetFunction: FunctionObject;
```

<a id="ref-q1-8"></a>
### [8] `src/intrinsics/FunctionPrototype.mts:47`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L47)

```
  readonly BoundThis: Value;
```

<a id="ref-q1-9"></a>
### [9] `src/intrinsics/FunctionPrototype.mts:48`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L48)

```
  readonly BoundArguments: Arguments;
```

<a id="ref-q1-10"></a>
### [10] `src/intrinsics/FunctionPrototype.mts:147-149`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L147-L149)

```
  if (!IsCallable(Target)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', Target);
  }
```

<a id="ref-q1-11"></a>
### [11] `src/intrinsics/FunctionPrototype.mts:151`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L151)

```
  // 3. Let F be ? BoundFunctionCreate(Target, thisArg, args).
```

<a id="ref-q1-12"></a>
### [12] `src/intrinsics/FunctionPrototype.mts:114-122`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L114-L122)

```
  const internalSlotsList = [
    'BoundTargetFunction',
    'BoundThis',
    'BoundArguments',
    'Prototype',
    'Extensible',
  ];
  // 4. Let obj be ! MakeBasicObject(internalSlotsList).
  const obj = X(MakeBasicObject(internalSlotsList)) as Mutable<BoundFunctionObject>;
```

<a id="ref-q1-13"></a>
### [13] `src/intrinsics/FunctionPrototype.mts:126`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L126)

```
  obj.Call = BoundFunctionExoticObjectCall;
```

<a id="ref-q1-14"></a>
### [14] `src/intrinsics/FunctionPrototype.mts:127-131`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L127-L131)

```
  // 7. If IsConstructor(targetFunction) is true, then
  if (IsConstructor(targetFunction)) {
    // a. Set obj.[[Construct]] as described in 9.4.1.2.
    obj.Construct = BoundFunctionExoticObjectConstruct;
  }
```

<a id="ref-q1-15"></a>
### [15] `src/intrinsics/FunctionPrototype.mts:133-136`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L133-L136)

```
  obj.BoundTargetFunction = targetFunction as FunctionObject;
  // 9. Set obj.[[BoundThis]] to boundThis.
  obj.BoundThis = boundThis;
  // 10. Set obj.[[BoundArguments]] to boundArguments.
```

<a id="ref-q1-16"></a>
### [16] `src/intrinsics/FunctionPrototype.mts:87-89`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L87-L89)

```
  const target = F.BoundTargetFunction;
  const boundThis = F.BoundThis;
  const boundArgs = F.BoundArguments;
```

<a id="ref-q1-17"></a>
### [17] `src/intrinsics/FunctionPrototype.mts:90-91`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L90-L91)

```
  const args = [...boundArgs, ...argumentsList];
  return Q(yield* Call(target, boundThis, args));
```

<a id="ref-q1-18"></a>
### [18] `src/intrinsics/FunctionPrototype.mts:98`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L98)

```
  Assert(IsConstructor(target));
```

<a id="ref-q1-19"></a>
### [19] `src/intrinsics/FunctionPrototype.mts:100-104`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L100-L104)

```
  const args = [...boundArgs, ...argumentsList];
  if (SameValue(F, newTarget) === Value.true) {
    newTarget = target;
  }
  return Q(yield* Construct(target, args, newTarget));
```

<a id="ref-q1-20"></a>
### [20] `src/intrinsics/FunctionPrototype.mts:195`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L195)

```
function* FunctionProto_call([thisArg = Value.undefined, ...args]: Arguments, { thisValue }: FunctionCallContext): ValueEvaluator {
```

<a id="ref-q1-21"></a>
### [21] `src/intrinsics/FunctionPrototype.mts:197-201`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L197-L201)

```
  const func = thisValue;
  // 2. If IsCallable(func) is false, throw a TypeError exception.
  if (!IsCallable(func)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', func);
  }
```

<a id="ref-q1-22"></a>
### [22] `src/intrinsics/FunctionPrototype.mts:202-211`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L202-L211)

```
  // 3. Let argList be a new empty List.
  const argList = [];
  // 4. If this method was called with more than one argument, then in left to right order, starting with the second argument, append each argument as the last element of argList.
  for (const arg of args) {
    argList.push(arg);
  }
  // 5. Perform PrepareForTailCall().
  PrepareForTailCall();
  // 6. Return ? Call(func, thisArg, argList).
  return Q(yield* Call(func, thisArg, argList));
```

<a id="ref-q1-23"></a>
### [23] `src/intrinsics/FunctionPrototype.mts:65-68`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L65-L68)

```
  // 2. If IsCallable(func) is false, throw a TypeError exception.
  if (!IsCallable(func)) {
    return surroundingAgent.Throw('TypeError', 'ThisNotAFunction', func);
  }
```

<a id="ref-q1-24"></a>
### [24] `src/intrinsics/FunctionPrototype.mts:69-75`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L69-L75)

```
  // 3. If argArray is undefined or null, then
  if (argArray === Value.undefined || argArray === Value.null) {
    // a. Perform PrepareForTailCall().
    PrepareForTailCall();
    // b. Return ? Call(func, thisArg).
    return Q(yield* Call(func, thisArg));
  }
```

<a id="ref-q1-25"></a>
### [25] `src/intrinsics/FunctionPrototype.mts:76-81`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L76-L81)

```
  // 4. Let argList be ? CreateListFromArrayLike(argArray).
  const argList = Q(yield* CreateListFromArrayLike(argArray));
  // 5. Perform PrepareForTailCall().
  PrepareForTailCall();
  // 6. Return ? Call(func, thisArg, argList).
  return Q(yield* Call(func, thisArg, argList));
```

<a id="ref-q1-26"></a>
### [26] `src/abstract-ops/testing-comparison.mts:60-69`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/testing-comparison.mts#L60-L69)

```
/** https://tc39.es/ecma262/#sec-iscallable */
export function IsCallable(argument: Value): argument is FunctionObject {
  if (!(argument instanceof ObjectValue)) {
    return false;
  }
  if ('Call' in argument) {
    return true;
  }
  return false;
}
```

<a id="ref-q1-27"></a>
### [27] `src/abstract-ops/testing-comparison.mts:63-67`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/testing-comparison.mts#L63-L67)

```
    return false;
  }
  if ('Call' in argument) {
    return true;
  }
```

<a id="ref-q1-28"></a>
### [28] `src/abstract-ops/function-operations.mts:498-504`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L498-L504)

```
  // 6. Return ! DefinePropertyOrThrow(F, "name", PropertyDescriptor { [[Value]]: name, [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }).
  X(DefinePropertyOrThrow(F, Value('name'), Descriptor({
    Value: name,
    Writable: Value.false,
    Enumerable: Value.false,
    Configurable: Value.true,
  })));
```

<a id="ref-q1-29"></a>
### [29] `src/abstract-ops/function-operations.mts:513-518`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L513-L518)

```
  X(DefinePropertyOrThrow(F, Value('length'), Descriptor({
    Value: toNumberValue(length),
    Writable: Value.false,
    Enumerable: Value.false,
    Configurable: Value.true,
  })));
```

<a id="ref-q1-30"></a>
### [30] `src/abstract-ops/function-operations.mts:462-505`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L462-L505)

```
export function SetFunctionName(F: FunctionObject, name: PropertyKeyValue | PrivateName, prefix?: JSStringValue): void {
  // 1. Assert: F is an extensible object that does not have a "name" own property.
  Assert(skipDebugger(IsExtensible(F)) === Value.true && skipDebugger(HasOwnProperty(F, Value('name'))) === Value.false);
  // 2. If Type(name) is Symbol, then
  if (name instanceof SymbolValue) {
    // a. Let description be name's [[Description]] value.
    const description = name.Description;
    // b. If description is undefined, set name to the empty String.
    if (description === Value.undefined) {
      name = Value('');
    } else {
      // c. Else, set name to the string-concatenation of "[", description, and "]".
      name = Value(`[${(description as JSStringValue).stringValue()}]`);
    }
  } else if (name instanceof PrivateName) { // 3. Else if name is a Private Name, then
    // a. Set name to name.[[Description]].
    name = name.Description;
  }
  // 4. If F has an [[InitialName]] internal slot, then
  if ('InitialName' in F) {
    // a. Set F.[[InitialName]] to name.
    (F as Mutable<FunctionObject>).InitialName = name;
  }
  if ('HostInitialName' in F) {
    // a. Set F.[[InitialName]] to name.
    (F as Mutable<ECMAScriptFunctionObject>).HostInitialName = name.stringValue();
  }
  // 5. If prefix is present, then
  if (prefix !== undefined) {
    // a. Set name to the string-concatenation of prefix, the code unit 0x0020 (SPACE), and name.
    name = Value(`${prefix.stringValue()} ${name.stringValue()}`);
    // b. If F has an [[InitialName]] internal slot, then
    if ('InitialName' in F) {
      // i. Optionally, set F.[[InitialName]] to name.
    }
  }
  // 6. Return ! DefinePropertyOrThrow(F, "name", PropertyDescriptor { [[Value]]: name, [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }).
  X(DefinePropertyOrThrow(F, Value('name'), Descriptor({
    Value: name,
    Writable: Value.false,
    Enumerable: Value.false,
    Configurable: Value.true,
  })));
}
```

<a id="ref-q1-31"></a>
### [31] `src/intrinsics/FunctionPrototype.mts:189`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L189)

```
  SetFunctionName(F, targetName, Value('bound'));
```

<a id="ref-q1-32"></a>
### [32] `src/abstract-ops/function-operations.mts:491-492`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L491-L492)

```
    // a. Set name to the string-concatenation of prefix, the code unit 0x0020 (SPACE), and name.
    name = Value(`${prefix.stringValue()} ${name.stringValue()}`);
```

<a id="ref-q1-33"></a>
### [33] `src/abstract-ops/function-operations.mts:508-519`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L508-L519)

```
export function SetFunctionLength(F: FunctionObject, length: number): void {
  Assert(isNonNegativeInteger(length) || length === Infinity);
  // 1. Assert: F is an extensible object that does not have a "length" own property.
  Assert(skipDebugger(IsExtensible(F)) === Value.true && skipDebugger(HasOwnProperty(F, Value('length'))) === Value.false);
  // 2. Return ! DefinePropertyOrThrow(F, "length", PropertyDescriptor { [[Value]]: 𝔽(length), [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }).
  X(DefinePropertyOrThrow(F, Value('length'), Descriptor({
    Value: toNumberValue(length),
    Writable: Value.false,
    Enumerable: Value.false,
    Configurable: Value.true,
  })));
}
```

<a id="ref-q1-34"></a>
### [34] `src/intrinsics/FunctionPrototype.mts:153-177`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L153-L177)

```
  // 4. Let L be 0.
  let L = 0;
  // 5. Let targetHasLength be ? HasOwnProperty(Target, "length").
  const targetHasLength = Q(yield* HasOwnProperty(Target, Value('length')));
  // 6. If targetHasLength is true, then
  if (targetHasLength === Value.true) {
    // a. Let targetLen be ? Get(Target, "length").
    const targetLen = Q(yield* Get(Target, Value('length')));
    // b. If Type(targetLen) is Number, then
    if (targetLen instanceof NumberValue) {
      // i. If targetLen is +∞𝔽, set L to +∞.
      if (R(targetLen) === +Infinity) {
        L = +Infinity;
      } else if (R(targetLen) === -Infinity) { // ii. Else if targetLen is -∞𝔽, set L to 0.
        L = 0;
      } else { // iii. Else,
        // 1. Set targetLen to ! ToIntegerOrInfinity(targetLen).
        const targetLenAsInt = Q(yield* ToIntegerOrInfinity(targetLen));
        // 2. Assert: targetLenAsInt is finite.
        Assert(Number.isFinite(targetLenAsInt));
        // 3. Let argCount be the number of elements in args.
        const argCount = args.length;
        // 4. Set L to max(targetLenAsInt - argCount, 0).
        L = Math.max(targetLenAsInt - argCount, 0);
      }
```

<a id="ref-q1-35"></a>
### [35] `src/intrinsics/FunctionPrototype.mts:180`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L180)

```
  // 7. Perform ! SetFunctionLength(F, L).
```

<a id="ref-q1-36"></a>
### [36] `src/intrinsics/FunctionPrototype.mts:268-270`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L268-L270)

```
    ['apply', FunctionProto_apply, 2],
    ['bind', FunctionProto_bind, 1],
    ['call', FunctionProto_call, 1],
```

<a id="ref-q1-37"></a>
### [37] `src/intrinsics/FunctionPrototype.mts:133-135`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/FunctionPrototype.mts#L133-L135)

```
  obj.BoundTargetFunction = targetFunction as FunctionObject;
  // 9. Set obj.[[BoundThis]] to boundThis.
  obj.BoundThis = boundThis;
```

<a id="ref-q1-38"></a>
### [38] `src/abstract-ops/testing-comparison.mts:43-58`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/testing-comparison.mts#L43-L58)

```
export function IsArray(argument: Value) {
  if (!(argument instanceof ObjectValue)) {
    return Value.false;
  }
  if (isArrayExoticObject(argument)) {
    return Value.true;
  }
  if (isProxyExoticObject(argument)) {
    if (argument.ProxyHandler === Value.null) {
      return surroundingAgent.Throw('TypeError', 'ProxyRevoked', 'IsArray');
    }
    const target = argument.ProxyTarget;
    return IsArray(target);
  }
  return Value.false;
}
```

<a id="ref-q1-39"></a>
### [39] `src/abstract-ops/function-operations.mts:568-610`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L568-L610)

```
export function CreateBuiltinFunction(steps: NativeSteps, length: number, name: PropertyKeyValue | PrivateName, internalSlotsList: readonly string[], realm?: Realm, prototype?: ObjectValue | NullValue, prefix?: JSStringValue, isConstructor: BooleanValue = Value.false): BuiltinFunctionObject {
  // 1. Assert: steps is either a set of algorithm steps or other definition of a function's behaviour provided in this specification.
  Assert(typeof steps === 'function');
  // 2. If realm is not present, set realm to the current Realm Record.
  if (realm === undefined) {
    realm = surroundingAgent.currentRealmRecord;
  }
  // 3. Assert: realm is a Realm Record.
  Assert(realm instanceof Realm);
  // 4. If prototype is not present, set prototype to realm.[[Intrinsics]].[[%Function.prototype%]].
  if (prototype === undefined) {
    prototype = realm.Intrinsics['%Function.prototype%'];
  }
  // 5. Let func be a new built-in function object that when called performs the action described by steps. The new function object has internal slots whose names are the elements of internalSlotsList.
  const func = X(MakeBasicObject(['Prototype', 'Extensible', 'Realm', 'ScriptOrModule', 'InitialName', 'IsClassConstructor'].concat(internalSlotsList))) as Mutable<BuiltinFunctionObject>;
  func.Call = BuiltinFunctionCall;
  if (isConstructor === Value.true) {
    func.Construct = BuiltinFunctionConstruct;
  }
  func.nativeFunction = steps;
  // 6. Set func.[[Realm]] to realm.
  func.Realm = realm;
  // 7. Set func.[[Prototype]] to prototype.
  func.Prototype = prototype;
  // 8. Set func.[[Extensible]] to true.
  func.Extensible = Value.true;
  // 10. Set func.[[InitialName]] to null.
  func.InitialName = Value.null;
  // https://github.com/tc39/ecma262/pull/3212/
  func.IsClassConstructor = Value.false;
  // 11. Perform ! SetFunctionLength(func, length).
  X(SetFunctionLength(func, length));
  // 12. If prefix is not present, then
  if (prefix === undefined) {
    // a. Perform ! SetFunctionName(func, name).
    X(SetFunctionName(func, name));
  } else { // 13. Else
    // a. Perform ! SetFunctionName(func, name, prefix).
    X(SetFunctionName(func, name, prefix));
  }
  // 13. Return func.
  return func;
}
```

<a id="ref-q1-40"></a>
### [40] `src/abstract-ops/function-operations.mts:583`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L583)

```
  func.Call = BuiltinFunctionCall;
```

<a id="ref-q1-41"></a>
### [41] `src/abstract-ops/function-operations.mts:521-522`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L521-L522)

```
function BuiltinFunctionCall(this: BuiltinFunctionObject, thisArgument: Value, argumentsList: Arguments): ValueEvaluator {
  return BuiltinCallOrConstruct(this, thisArgument, argumentsList, Value.undefined);
```

<a id="ref-q1-42"></a>
### [42] `src/abstract-ops/function-operations.mts:531-565`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/function-operations.mts#L531-L565)

```
function* BuiltinCallOrConstruct(F: BuiltinFunctionObject, thisArgument: Value | 'uninitialized', argumentsList: Arguments, newTarget: FunctionObject | UndefinedValue): ValueEvaluator {
  const calleeContext = new ExecutionContext();
  calleeContext.Function = F;
  const calleeRealm = F.Realm;
  calleeContext.Realm = calleeRealm;
  calleeContext.ScriptOrModule = Value.null;
  surroundingAgent.executionContextStack.push(calleeContext);

  const isNew = thisArgument === 'uninitialized';
  // Perform any necessary implementation-defined initialization of calleeContext.
  surroundingAgent.runningExecutionContext.callSite.constructCall = isNew;

  let completion = F.nativeFunction(argumentsList, {
    thisValue: thisArgument === 'uninitialized' ? Value.undefined : thisArgument,
    NewTarget: newTarget,
  });
  // in case of debugging, use the following version so F.nativeFunction's name can appears in the stack trace.
  // let completion = Reflect['apply'](F.nativeFunction, F, [argumentsList, {
  //   thisValue: thisArgument || Value.undefined,
  //   NewTarget: newTarget || Value.undefined,
  // }]);
  if (completion && 'next' in completion) {
    completion = yield* completion;
  }
  if (completion instanceof Completion) {
    Assert(completion instanceof NormalCompletion || completion instanceof ThrowCompletion);
  }

  surroundingAgent.executionContextStack.pop(calleeContext);
  const result = Q(completion) || Value.undefined;
  if (isNew) {
    Assert(result instanceof ObjectValue);
  }
  return result;
}
```

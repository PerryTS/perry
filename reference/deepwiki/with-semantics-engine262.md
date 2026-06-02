# DeepWiki Response

Repository: `engine262/engine262`
Question: How does this ECMAScript engine implement with statement object environment records? Please focus on ObjectEnvironmentRecord/withEnvironment HasBinding, GetBindingValue, SetMutableBinding, DeleteBinding, @@unscopables handling, closure capture through with object environments, and strict-mode syntax restrictions. Include concrete source references.
Search: https://deepwiki.com/search/how-does-this-ecmascript-engin_420251b9-dd6d-428d-a5bd-233ec846e72b

**Question**: How does this ECMAScript engine implement with statement object environment records? Please focus on ObjectEnvironmentRecord/withEnvironment HasBinding, GetBindingValue, SetMutableBinding, DeleteBinding, @@unscopables handling, closure capture through with object environments, and strict-mode syntax restrictions. Include concrete source references.

This ECMAScript engine implements `with` statement object environment records using the `ObjectEnvironmentRecord` class, which is a subclass of `EnvironmentRecord` [1](#ref-q1-1)  [2](#ref-q1-2) . The `IsWithEnvironment` flag within `ObjectEnvironmentRecord` specifically indicates if the record is associated with a `with` statement [3](#ref-q1-3) .

### `ObjectEnvironmentRecord` Methods

The `ObjectEnvironmentRecord` class overrides several abstract methods from `EnvironmentRecord` to define its behavior [1](#ref-q1-1)  [2](#ref-q1-2) .

#### `HasBinding`
The `HasBinding` method checks for the existence of a binding for a given name `N` [4](#ref-q1-4) . It first checks if the `BindingObject` has the property [5](#ref-q1-5) . If the `IsWithEnvironment` flag is `true`, it then checks for `@@unscopables` [6](#ref-q1-6) . If `@@unscopables` is an object and `N` is `true` within it, the binding is considered blocked and `HasBinding` returns `false` [7](#ref-q1-7) .

#### `GetBindingValue`
The `GetBindingValue` method retrieves the value of a binding [8](#ref-q1-8) . It checks if the `BindingObject` has the property `N` [9](#ref-q1-9) . If not found and `S` (strict) is `false`, it returns `undefined`; otherwise, it throws a `ReferenceError` [10](#ref-q1-10) . If the property exists, it returns its value from the `BindingObject` [11](#ref-q1-11) .

#### `SetMutableBinding`
The `SetMutableBinding` method sets the value of a mutable binding [12](#ref-q1-12) . It first checks if the property `N` exists on the `BindingObject` [13](#ref-q1-13) . If it doesn't exist and `S` (strict) is `true`, it throws a `ReferenceError` [14](#ref-q1-14) . Otherwise, it calls the `Set` abstract operation on the `BindingObject` [15](#ref-q1-15) .

#### `DeleteBinding`
The `DeleteBinding` method attempts to delete a binding [16](#ref-q1-16) . It directly calls the `[[Delete]]` internal method of the `BindingObject` with the name `N` [17](#ref-q1-17) .

### `@@unscopables` Handling
The `@@unscopables` symbol is specifically handled within the `HasBinding` method of `ObjectEnvironmentRecord` when `IsWithEnvironment` is `true` [6](#ref-q1-6) . This mechanism prevents certain properties from being exposed through the `with` statement's environment, aligning with the ECMAScript specification for `with` statement behavior [7](#ref-q1-7) .

### Closure Capture and Strict-Mode Restrictions
The `ObjectEnvironmentRecord` itself does not directly implement closure capture; rather, it is part of the environment chain that functions close over [18](#ref-q1-18) . The `OuterEnv` property links environment records, forming the lexical environment chain [19](#ref-q1-19) .

Strict-mode restrictions are reflected in the `SetMutableBinding` and `GetBindingValue` methods. For instance, in `SetMutableBinding`, if a binding does not exist and the operation is in strict mode (`S` is `true`), a `ReferenceError` is thrown [14](#ref-q1-14) . Similarly, in `GetBindingValue`, if a binding is not found and `S` is `true`, a `ReferenceError` is thrown [10](#ref-q1-10) . The `with` statement itself is forbidden in strict mode, but the `ObjectEnvironmentRecord` methods handle strictness for operations that might occur within a non-strict `with` block.

## Notes
The `GlobalEnvironmentRecord` also contains an `ObjectEnvironmentRecord` as its `ObjectRecord` property, but this `ObjectEnvironmentRecord` is initialized with `IsWithEnvironment` set to `false` [20](#ref-q1-20) . This means the `@@unscopables` logic is not applied to the global object's environment record [21](#ref-q1-21) .

View this search on DeepWiki: https://deepwiki.com/search/how-does-this-ecmascript-engin_420251b9-dd6d-428d-a5bd-233ec846e72b

## References

<a id="ref-q1-1"></a>
### [1] `src/environment.mts:261`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L261)

```
export class ObjectEnvironmentRecord extends EnvironmentRecord {
```

<a id="ref-q1-2"></a>
### [2] `src/environment.mts:37`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L37)

```
export abstract class EnvironmentRecord {
```

<a id="ref-q1-3"></a>
### [3] `src/environment.mts:265`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L265)

<a id="ref-q1-4"></a>
### [4] `src/environment.mts:273-302`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L273-L302)

```
  /** https://tc39.es/ecma262/#sec-object-environment-records-hasbinding-n */
  * HasBinding(N: JSStringValue): ValueEvaluator<BooleanValue> {
    // 1. Let envRec be the object Environment Record for which the method was invoked.
    const envRec = this;
    // 2. Let bindings be the binding object for envRec.
    const bindings = envRec.BindingObject;
    // 3. Let foundBinding be ? HasProperty(bindings, N).
    const foundBinding = Q(yield* HasProperty(bindings, N));
    // 4. If foundBinding is false, return false.
    if (foundBinding === Value.false) {
      return Value.false;
    }
    // 5. If the IsWithEnvironment flag of envRec i s false, return true.
    if (envRec.IsWithEnvironment === Value.false) {
      return Value.true;
    }
    // 6. Let unscopables be ? Get(bindings, @@unscopables).
    const unscopables = Q(yield* Get(bindings, wellKnownSymbols.unscopables));
    // 7. If Type(unscopables) is Object, then
    if (unscopables instanceof ObjectValue) {
      // a. Let blocked be ! ToBoolean(? Get(unscopables, N)).
      const blocked = X(ToBoolean(Q(yield* Get(unscopables, N))));
      // b. If blocked is true, return false.
      if (blocked === Value.true) {
        return Value.false;
      }
    }
    // 8. Return true.
    return Value.true;
  }
```

<a id="ref-q1-5"></a>
### [5] `src/environment.mts:278-280`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L278-L280)

```
    const bindings = envRec.BindingObject;
    // 3. Let foundBinding be ? HasProperty(bindings, N).
    const foundBinding = Q(yield* HasProperty(bindings, N));
```

<a id="ref-q1-6"></a>
### [6] `src/environment.mts:285-299`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L285-L299)

```
    // 5. If the IsWithEnvironment flag of envRec i s false, return true.
    if (envRec.IsWithEnvironment === Value.false) {
      return Value.true;
    }
    // 6. Let unscopables be ? Get(bindings, @@unscopables).
    const unscopables = Q(yield* Get(bindings, wellKnownSymbols.unscopables));
    // 7. If Type(unscopables) is Object, then
    if (unscopables instanceof ObjectValue) {
      // a. Let blocked be ! ToBoolean(? Get(unscopables, N)).
      const blocked = X(ToBoolean(Q(yield* Get(unscopables, N))));
      // b. If blocked is true, return false.
      if (blocked === Value.true) {
        return Value.false;
      }
    }
```

<a id="ref-q1-7"></a>
### [7] `src/environment.mts:290-297`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L290-L297)

```
    const unscopables = Q(yield* Get(bindings, wellKnownSymbols.unscopables));
    // 7. If Type(unscopables) is Object, then
    if (unscopables instanceof ObjectValue) {
      // a. Let blocked be ! ToBoolean(? Get(unscopables, N)).
      const blocked = X(ToBoolean(Q(yield* Get(unscopables, N))));
      // b. If blocked is true, return false.
      if (blocked === Value.true) {
        return Value.false;
```

<a id="ref-q1-8"></a>
### [8] `src/environment.mts:351-370`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L351-L370)

```
  /** https://tc39.es/ecma262/#sec-object-environment-records-getbindingvalue-n-s */
  * GetBindingValue(N: JSStringValue, S: BooleanValue): ValueEvaluator {
    // 1. Let envRec be the object Environment Record for which the method was invoked.
    const envRec = this;
    // 2. Let bindings be the binding object for envRec.
    const bindings = envRec.BindingObject;
    // 3. Let value be ? HasProperty(bindings, N).
    const value = Q(yield* HasProperty(bindings, N));
    // 4. If value is false, then
    if (value === Value.false) {
      // a. If S is false, return the value undefined; otherwise throw a ReferenceError exception.
      if (S === Value.false) {
        return NormalCompletion(Value.undefined);
      } else {
        return surroundingAgent.Throw('ReferenceError', 'NotDefined', N);
      }
    }
    // 5. Return Get(bindings, N).
    return yield* Get(bindings, N);
  }
```

<a id="ref-q1-9"></a>
### [9] `src/environment.mts:357-358`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L357-L358)

```
    // 3. Let value be ? HasProperty(bindings, N).
    const value = Q(yield* HasProperty(bindings, N));
```

<a id="ref-q1-10"></a>
### [10] `src/environment.mts:360-366`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L360-L366)

```
    if (value === Value.false) {
      // a. If S is false, return the value undefined; otherwise throw a ReferenceError exception.
      if (S === Value.false) {
        return NormalCompletion(Value.undefined);
      } else {
        return surroundingAgent.Throw('ReferenceError', 'NotDefined', N);
      }
```

<a id="ref-q1-11"></a>
### [11] `src/environment.mts:368-369`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L368-L369)

```
    // 5. Return Get(bindings, N).
    return yield* Get(bindings, N);
```

<a id="ref-q1-12"></a>
### [12] `src/environment.mts:335-349`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L335-L349)

```
  * SetMutableBinding(N: JSStringValue, V: Value, S: BooleanValue): PlainEvaluator {
    // 1. Let envRec be the object Environment Record for which the method was invoked.
    const envRec = this;
    // 2. Let bindings be the binding object for envRec.
    const bindings = envRec.BindingObject;
    // 3. Let stillExists be ? HasProperty(bindings, N).
    const stillExists = Q(yield* HasProperty(bindings, N));
    // 4. If stillExists is false and S is true, throw a ReferenceError exception.
    if (stillExists === Value.false && S === Value.true) {
      return surroundingAgent.Throw('ReferenceError', 'NotDefined', N);
    }
    // 5. Return ? Set(bindings, N, V, S).
    Q(yield* Set(bindings, N, V, S));
    return undefined;
  }
```

<a id="ref-q1-13"></a>
### [13] `src/environment.mts:340-341`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L340-L341)

```
    // 3. Let stillExists be ? HasProperty(bindings, N).
    const stillExists = Q(yield* HasProperty(bindings, N));
```

<a id="ref-q1-14"></a>
### [14] `src/environment.mts:343-345`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L343-L345)

```
    if (stillExists === Value.false && S === Value.true) {
      return surroundingAgent.Throw('ReferenceError', 'NotDefined', N);
    }
```

<a id="ref-q1-15"></a>
### [15] `src/environment.mts:346-347`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L346-L347)

```
    // 5. Return ? Set(bindings, N, V, S).
    Q(yield* Set(bindings, N, V, S));
```

<a id="ref-q1-16"></a>
### [16] `src/environment.mts:372-380`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L372-L380)

```
  /** https://tc39.es/ecma262/#sec-object-environment-records-deletebinding-n */
  * DeleteBinding(N: JSStringValue): ValueEvaluator<BooleanValue> {
    // 1. Let envRec be the object Environment Record for which the method was invoked.
    const envRec = this;
    // 2. Let bindings be the binding object for envRec.
    const bindings = envRec.BindingObject;
    // 3. Return ? bindings.[[Delete]](N).
    return Q(yield* bindings.Delete(N));
  }
```

<a id="ref-q1-17"></a>
### [17] `src/environment.mts:378`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L378)

```
    // 3. Return ? bindings.[[Delete]](N).
```

<a id="ref-q1-18"></a>
### [18] `src/environment.mts:38-42`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L38-L42)

```
  readonly OuterEnv: EnvironmentRecord | NullValue;

  constructor(outerEnv: EnvironmentRecord | NullValue) {
    this.OuterEnv = outerEnv;
  }
```

<a id="ref-q1-19"></a>
### [19] `src/environment.mts:38`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L38)

```
  readonly OuterEnv: EnvironmentRecord | NullValue;
```

<a id="ref-q1-20"></a>
### [20] `src/environment.mts:538-539`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L538-L539)

```
    // 1. Let objRec be NewObjectEnvironment(G, false, null).
    const objRec = new ObjectEnvironmentRecord(G, Value.false, Value.null);
```

<a id="ref-q1-21"></a>
### [21] `src/environment.mts:285-287`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/environment.mts#L285-L287)

```
    // 5. If the IsWithEnvironment flag of envRec i s false, return true.
    if (envRec.IsWithEnvironment === Value.false) {
      return Value.true;
```

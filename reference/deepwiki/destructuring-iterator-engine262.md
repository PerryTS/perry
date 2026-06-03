# DeepWiki Response

Repository: `engine262/engine262`
Question: For ECMAScript destructuring assignment, how does this implementation handle IteratorClose and target evaluation order? Please focus on array/object assignment destructuring, abrupt completions from target evaluation or assignment, iterator.return error precedence, and cite the relevant implementation files/functions.
Search: https://deepwiki.com/search/for-ecmascript-destructuring-a_07f50f08-1a0b-40c8-9418-c3289a38e7e4

**Question**: For ECMAScript destructuring assignment, how does this implementation handle IteratorClose and target evaluation order? Please focus on array/object assignment destructuring, abrupt completions from target evaluation or assignment, iterator.return error precedence, and cite the relevant implementation files/functions.

This implementation handles `IteratorClose` and target evaluation order in ECMAScript destructuring assignments by using specific macros and abstract operations to manage abrupt completions and ensure proper iterator cleanup. The core mechanisms involve the `Q` macro for propagating abrupt completions, and `IfAbruptCloseIterator` for handling iterator closing when an abrupt completion occurs during iteration or target evaluation.

### IteratorClose and Abrupt Completions

The `IteratorClose` abstract operation is crucial for ensuring that iterators are properly closed, especially when an abrupt completion (like an error) occurs during iteration [1](#ref-q1-1) . When an abrupt completion `completion` is passed to `IteratorClose`, the `completion` is returned directly, prioritizing the original abrupt completion [2](#ref-q1-2) . If the `iterator.return` method itself throws an error, that error takes precedence and is returned [3](#ref-q1-3) .

The `IfAbruptCloseIterator` macro is used to integrate this behavior into various operations that consume iterators [4](#ref-q1-4) . This macro checks if a `value` is an `AbruptCompletion`. If it is, `IteratorClose` is called with the `iteratorRecord` and the `AbruptCompletion`, and the result is returned [5](#ref-q1-5) . This ensures that if any step in the destructuring process results in an abrupt completion, the iterator is closed before propagating the error.

### Target Evaluation Order and Abrupt Completions

Destructuring assignments, particularly for arrays and objects, involve evaluating assignment targets. The `engine262` implementation uses a macro system to handle abrupt completions during these evaluations. The `Q` macro (short for `ReturnIfAbrupt`) is a fundamental mechanism for propagating abrupt completions [6](#ref-q1-6) . If an expression evaluated using `Q` results in an `AbruptCompletion`, that completion is immediately returned, effectively short-circuiting further evaluation [7](#ref-q1-7) .

For example, in `Array.from`, when iterating over items, `IteratorStepValue` is called using `Q` [8](#ref-q1-8) . If `IteratorStepValue` produces an abrupt completion, the `Q` macro will cause it to be returned. Subsequently, `IfAbruptCloseIterator` is used to ensure the iterator is closed if a mapped value results in an abrupt completion [9](#ref-q1-9) .

The `X` macro is similar to `Q` but asserts that the completion must be normal; if it's an `AbruptCompletion`, it throws an `Assert.Error` [10](#ref-q1-10) . This is used in scenarios where an abrupt completion at that point would indicate an internal engine error rather than a recoverable JavaScript error [11](#ref-q1-11) .

### Array and Object Assignment Destructuring

The parsing of array and object assignment patterns is defined in `src/parser/ParseNode.mts` [12](#ref-q1-12) . These definitions include `ObjectAssignmentPattern`, `ArrayAssignmentPattern`, `AssignmentProperty`, `AssignmentElement`, and `AssignmentRestProperty`. The runtime semantics for these are handled by operations like `DestructuringAssignmentEvaluation` [13](#ref-q1-13) .

The evaluation order for destructuring assignments generally follows the order of elements in the pattern. Each element's evaluation can potentially lead to an abrupt completion, which would then be propagated by the `Q` macro, and iterators would be closed via `IfAbruptCloseIterator` if applicable.

### Iterator.return Error Precedence

The `IteratorClose` abstract operation explicitly defines the precedence of errors [1](#ref-q1-1) . If the `completion` argument passed to `IteratorClose` is an `AbruptCompletion`, that completion is preserved and returned [2](#ref-q1-2) . However, if the `iterator.return` method itself throws an error, that new error takes precedence and is returned instead [3](#ref-q1-3) . This ensures that errors originating from the iterator's cleanup process are not silently discarded.

## Notes

The provided context primarily focuses on the internal mechanisms for handling completions and iterator operations. While `src/parser/ParseNode.mts` [12](#ref-q1-12)  and `src/parser/ExpressionParser.mts` [14](#ref-q1-14)  define the structure and validation of destructuring patterns, the detailed runtime semantics for how each part of a destructuring assignment is evaluated and how errors are propagated during that evaluation are handled by the abstract operations and macros described. The specific implementation of `DestructuringAssignmentEvaluation` was not provided in the snippets, but its presence in `src/runtime-semantics/all.mts` [13](#ref-q1-13)  indicates where these detailed semantics would reside.

Wiki pages you might want to explore:
- [Core Architecture (engine262/engine262)](/wiki/engine262/engine262#3)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-destructuring-a_07f50f08-1a0b-40c8-9418-c3289a38e7e4

## References

<a id="ref-q1-1"></a>
### [1] `src/abstract-ops/iterator-operations.mts:185-207`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/iterator-operations.mts#L185-L207)

```
/** https://tc39.es/ecma262/#sec-iteratorclose */
export function* IteratorClose<T, C extends Completion<T>>(iteratorRecord: IteratorRecord, completion: C): Evaluator<C | ThrowCompletion> {
  Assert(iteratorRecord.Iterator instanceof ObjectValue);
  const iterator = iteratorRecord.Iterator;
  let innerResult: ValueCompletion = EnsureCompletion(yield* GetMethod(iterator, Value('return')));
  if (innerResult instanceof NormalCompletion) {
    const ret = innerResult.Value;
    if (ret === Value.undefined) {
      return completion;
    }
    innerResult = EnsureCompletion(yield* Call(ret, iterator));
  }
  if (completion instanceof ThrowCompletion) {
    return completion;
  }
  if (innerResult instanceof ThrowCompletion) {
    return innerResult;
  }
  if (!(innerResult.Value instanceof ObjectValue)) {
    return surroundingAgent.Throw('TypeError', 'NotAnObject', innerResult.Value);
  }
  return completion;
}
```

<a id="ref-q1-2"></a>
### [2] `src/abstract-ops/iterator-operations.mts:197-199`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/iterator-operations.mts#L197-L199)

```
  if (completion instanceof ThrowCompletion) {
    return completion;
  }
```

<a id="ref-q1-3"></a>
### [3] `src/abstract-ops/iterator-operations.mts:200-202`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/abstract-ops/iterator-operations.mts#L200-L202)

```
  if (innerResult instanceof ThrowCompletion) {
    return innerResult;
  }
```

<a id="ref-q1-4"></a>
### [4] `scripts/transform.mts:156-165`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/scripts/transform.mts#L156-L165)

```
    IfAbruptCloseIterator: {
      template: template(`
      /* IfAbruptCloseIterator */
      /* node:coverage ignore next */
      if (%%value%% instanceof AbruptCompletion) return skipDebugger(IteratorClose(%%iteratorRecord%%, %%value%%));
      /* node:coverage ignore next */
      if (%%value%% instanceof Completion) %%value%% = %%value%%.Value;
      `, { preserveComments: true }),
      imports: ['IteratorClose', 'AbruptCompletion', 'Completion', 'skipDebugger'],
    },
```

<a id="ref-q1-5"></a>
### [5] `scripts/transform.mts:159-160`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/scripts/transform.mts#L159-L160)

```
      /* node:coverage ignore next */
      if (%%value%% instanceof AbruptCompletion) return skipDebugger(IteratorClose(%%iteratorRecord%%, %%value%%));
```

<a id="ref-q1-6"></a>
### [6] `scripts/transform.mts:136-145`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/scripts/transform.mts#L136-L145)

```
    Q: {
      template: template(`
      /* ReturnIfAbrupt */
      %%checkYieldStar%%
      /* node:coverage ignore next */ if (%%value%% instanceof AbruptCompletion) return %%value%%;
      /* node:coverage ignore next */ if (%%value%% instanceof Completion) %%value%% = %%value%%.Value;
      `, { preserveComments: true }),
      imports: ['AbruptCompletion', 'Completion', 'Assert'],
      allowAnyExpression: true,
    },
```

<a id="ref-q1-7"></a>
### [7] `scripts/transform.mts:139-140`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/scripts/transform.mts#L139-L140)

```
      %%checkYieldStar%%
      /* node:coverage ignore next */ if (%%value%% instanceof AbruptCompletion) return %%value%%;
```

<a id="ref-q1-8"></a>
### [8] `src/intrinsics/Array.mts:141`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/Array.mts#L141)

```
      const next = Q(yield* IteratorStepValue(iteratorRecord));
```

<a id="ref-q1-9"></a>
### [9] `src/intrinsics/Array.mts:148-149`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/intrinsics/Array.mts#L148-L149)

```
        mappedValue = yield* Call(mapper, thisArg, [next, F(k)]);
        IfAbruptCloseIterator(mappedValue, iteratorRecord);
```

<a id="ref-q1-10"></a>
### [10] `scripts/transform.mts:146-155`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/scripts/transform.mts#L146-L155)

```
    X: {
      template: template(`
      /* X */
      %%checkYieldStar%%
      /* node:coverage ignore next */ if (%%value%% instanceof AbruptCompletion) throw new Assert.Error(%%source%%, { cause: %%value%% });
      /* node:coverage ignore next */ if (%%value%% instanceof Completion) %%value%% = %%value%%.Value;
      `, { preserveComments: true }),
      imports: ['Assert', 'Completion', 'AbruptCompletion', 'skipDebugger'],
      allowAnyExpression: true,
    },
```

<a id="ref-q1-11"></a>
### [11] `src/completion.mts:359-360`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/completion.mts#L359-L360)

```
  /* node:coverage ignore next */
  throw new Assert.Error('Unexpected AbruptCompletion.', { cause: c });
```

<a id="ref-q1-12"></a>
### [12] `src/parser/ParseNode.mts:2180-2210`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ParseNode.mts#L2180-L2210)

```
  export type AssignmentPattern = ObjectAssignmentPattern | ArrayAssignmentPattern | AssignmentProperty | AssignmentElement | ParseNode.Elision;
  export type ObjectAssignmentPattern = {
    type: 'ObjectAssignmentPattern';
    AssignmentPropertyList: (AssignmentProperty | AssignmentPattern)[];
    AssignmentRestProperty: AssignmentRestProperty | undefined;
  }
  export type AssignmentProperty = {
    type: 'AssignmentProperty';
    IdentifierReference: ParseNode.IdentifierReference;
    Initializer?: ParseNode.Initializer | null | undefined;
  } | {
    type: 'AssignmentProperty';
    PropertyName: ParseNode.PropertyNameLike | null;
    AssignmentElement: AssignmentElement;
  }
  export type AssignmentElement = {
    type: 'AssignmentElement';
    DestructuringAssignmentTarget: ParseNode.AssignmentExpressionOrHigher;
    Initializer: ParseNode.Initializer | undefined | null;
  }

  export type ArrayAssignmentPattern = {
    type: 'ArrayAssignmentPattern';
    AssignmentElementList: AssignmentElisionElement[];
    AssignmentRestElement: AssignmentRestElement | undefined;
  }
  export type AssignmentElisionElement = ParseNode.Elision | AssignmentElement | AssignmentPattern;
  export type AssignmentRestProperty = {
    type: 'AssignmentRestProperty';
    DestructuringAssignmentTarget: ParseNode.AssignmentExpressionOrHigher;
  }
```

<a id="ref-q1-13"></a>
### [13] `src/runtime-semantics/all.mts:97`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/runtime-semantics/all.mts#L97)

```
export * from './DestructuringAssignmentEvaluation.mts';
```

<a id="ref-q1-14"></a>
### [14] `src/parser/ExpressionParser.mts:170-180`
Source: [engine262/engine262 @ 92b97644](https://github.com/engine262/engine262/blob/92b97644/src/parser/ExpressionParser.mts#L170-L180)

```
        node.ElementList.forEach((p, i) => {
          if (p.type === 'SpreadElement' && (i !== node.ElementList.length - 1 || node.hasTrailingComma)) {
            this.raiseEarly('InvalidAssignmentTarget', p);
          }
          if (p.type === 'AssignmentExpression') {
            this.validateAssignmentTarget(p.LeftHandSideExpression);
          } else {
            this.validateAssignmentTarget(p);
          }
        });
        return;
```

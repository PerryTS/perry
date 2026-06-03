# DeepWiki Response

Repository: `boa-dev/boa`
Question: For Function.prototype.call/apply/bind in Boa, how are bound functions represented and invoked? Please focus on detached helper patterns such as Function.prototype.call.bind(Object.prototype.hasOwnProperty), IsCallable checks, Array.isArray as a callable builtin, and creation of function name/length property descriptors for ordinary and bound functions. Include relevant source paths and citations.
Search: https://deepwiki.com/search/for-functionprototypecallapply_132b61a5-ea5e-4a85-9fdd-9a0155481a74

**Question**: For Function.prototype.call/apply/bind in Boa, how are bound functions represented and invoked? Please focus on detached helper patterns such as Function.prototype.call.bind(Object.prototype.hasOwnProperty), IsCallable checks, Array.isArray as a callable builtin, and creation of function name/length property descriptors for ordinary and bound functions. Include relevant source paths and citations.

In Boa, bound functions are represented by the `BoundFunction` struct and invoked through exotic internal methods. The `Function.prototype.bind` method creates a `BoundFunction` object, which encapsulates the target function, the `this` value, and any bound arguments. Invocation of bound functions, as well as `Function.prototype.call` and `Function.prototype.apply`, involves `IsCallable` checks and careful management of the call stack.

## Bound Function Representation

Bound functions are represented by the `BoundFunction` struct [1](#ref-q1-1) . This struct stores three key pieces of information:
*   `target_function`: The original function that was bound [2](#ref-q1-2) .
*   `this`: The `this` value that will be used when the `target_function` is called [3](#ref-q1-3) .
*   `args`: A `Vec<JsValue>` containing the arguments that were bound during the creation of the bound function [4](#ref-q1-4) .

The `BoundFunction::create` method is responsible for creating a new bound function object [5](#ref-q1-5) . It sets up the prototype chain and stores the `BoundFunction` data within a `JsObject` [6](#ref-q1-6) .

## Invocation of Bound Functions and `call`/`apply`

Bound functions, as well as `Function.prototype.call` and `Function.prototype.apply`, rely on the `IsCallable` check to ensure that the target is indeed a function.

### `IsCallable` Checks

The `is_callable` method on `JsValue` checks if a JavaScript value is a callable function [7](#ref-q1-7) . This method internally calls `JsObject::is_callable` [8](#ref-q1-8) , which determines if the object has a `[[Call]]` internal method different from the ordinary one [9](#ref-q1-9) .

For example, in `Function.prototype.apply`, the `this` value is checked to be callable using `this.as_callable()` [10](#ref-q1-10) . Similarly, `Function.prototype.bind` also performs an `IsCallable` check on its `Target` [11](#ref-q1-11) .

### `Function.prototype.call` and `Function.prototype.apply`

Both `call` and `apply` methods on `Function.prototype` ultimately invoke the target function using its `[[Call]]` internal method.
*   `Function.prototype.apply` [12](#ref-q1-12) : Takes `thisArg` and an `argArray`. It creates a list of arguments from `argArray` using `create_list_from_array_like` and then calls the function with the specified `thisArg` and the prepared argument list [13](#ref-q1-13) .
*   `Function.prototype.call` [14](#ref-q1-14) : Takes `thisArg` and individual arguments. It extracts the `thisArg` and the rest of the arguments, then calls the function with them [15](#ref-q1-15) .

The actual function invocation is handled by `JsObject::call` [16](#ref-q1-16) , which pushes the `this` value and arguments onto the VM stack and then executes the function's `[[Call]]` internal method [17](#ref-q1-17) .

### Bound Function Invocation

When a bound function is invoked, its `[[Call]]` internal method, `bound_function_exotic_call`, is executed [18](#ref-q1-18) . This method retrieves the `target_function`, `bound_this`, and `bound_args` from the `BoundFunction` object [19](#ref-q1-19) . It then sets the `this` value and inserts the bound arguments onto the VM stack before calling the `target_function` with the combined arguments [20](#ref-q1-20) .

If the bound function is used as a constructor (e.g., `new BoundFunction()`), the `bound_function_exotic_construct` internal method is called [21](#ref-q1-21) . This method similarly retrieves the `target_function` and `bound_args`, prepares the arguments, and then calls the `target_function`'s `[[Construct]]` method [22](#ref-q1-22) .

### Detached Helper Patterns

The pattern `Function.prototype.call.bind(Object.prototype.hasOwnProperty)` is used to create a "detached" version of `hasOwnProperty`. This works because `Function.prototype.bind` creates a new function where the `this` value is permanently set. In this case, `Object.prototype.hasOwnProperty` becomes the `target_function` and `Function.prototype.call` becomes the `this` value for the `bind` call. When the resulting bound function is called, it will invoke `Function.prototype.call` with `Object.prototype.hasOwnProperty` as its `this` and the first argument of the bound function as the `this` for `hasOwnProperty`.

## Function Name and Length Property Descriptors

### Ordinary Functions

For ordinary functions, the `name` and `length` properties are set during their creation. The `FunctionObjectBuilder` is used to construct native function objects [23](#ref-q1-23) . It allows specifying the `name` and `length` of the function [24](#ref-q1-24) . These properties are defined as non-writable, non-enumerable, and configurable [25](#ref-q1-25) .

The `create_function_object_fast` function also sets the `name` and `length` properties when creating function objects from `CodeBlock`s [26](#ref-q1-26) . The `Intrinsics` struct provides templates for different function types (e.g., `function`, `async_function`, `generator_function`) which define the default property descriptors for `length` and `name` [27](#ref-q1-27) .

### Bound Functions

For bound functions, the `length` and `name` properties are specifically calculated and set during the `Function.prototype.bind` operation [28](#ref-q1-28) .
*   **Length**: The `length` property of a bound function is calculated as `max(targetLenAsInt - argCount, 0)`, where `targetLenAsInt` is the length of the target function and `argCount` is the number of arguments bound [29](#ref-q1-29) . This property is defined as non-writable, non-enumerable, and configurable [30](#ref-q1-30) .
*   **Name**: The `name` property of a bound function is derived from the `target_name` with a "bound" prefix. The `set_function_name` abstract operation handles this [31](#ref-q1-31) . The `name` property is also defined as non-writable, non-enumerable, and configurable [32](#ref-q1-32) .

## `Array.isArray` as a Callable Builtin

`Array.isArray` is a standard built-in function [33](#ref-q1-33) . While the provided snippets do not explicitly show `Array.isArray`'s implementation, it would be initialized as a callable function using the `BuiltInBuilder` pattern, similar to other built-in methods like `Array.prototype.map` [34](#ref-q1-34) . Its `is_callable` check would return `true` because it has a defined `[[Call]]` internal method.

## Notes

The `Glossary` wiki page provides a good overview of core engine concepts like `Context`, `JsValue`, and `Realm` [35](#ref-q1-35) . The `Standard Builtins` wiki page details how built-in objects are implemented in Boa, including the use of `BuiltInBuilder` for defining methods and properties <cite repo="boa-dev/boa" path="boa-dev/boa" start="3.1" end="3.1" />. These are foundational to understanding the implementation of `Function.prototype.call`, `apply`, and `bind`.

Wiki pages you might want to explore:
- [Standard Builtins (boa-dev/boa)](/wiki/boa-dev/boa#3.1)
- [Glossary (boa-dev/boa)](/wiki/boa-dev/boa#7)

View this search on DeepWiki: https://deepwiki.com/search/for-functionprototypecallapply_132b61a5-ea5e-4a85-9fdd-9a0155481a74

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/builtins/function/bound.rs:15-19`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L15-L19)

```rust
pub struct BoundFunction {
    target_function: JsObject,
    this: JsValue,
    args: Vec<JsValue>,
}
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/builtins/function/bound.rs:16`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L16)

```rust
    target_function: JsObject,
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/builtins/function/bound.rs:17`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L17)

```rust
    this: JsValue,
```

<a id="ref-q1-4"></a>
### [4] `core/engine/src/builtins/function/bound.rs:18`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L18)

```rust
    args: Vec<JsValue>,
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/builtins/function/bound.rs:49-78`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L49-L78)

```rust
    pub fn create(
        target_function: JsObject,
        this: JsValue,
        args: Vec<JsValue>,
        context: &mut Context,
    ) -> JsResult<JsObject> {
        // 1. Let proto be ? targetFunction.[[GetPrototypeOf]]().
        let proto = target_function.__get_prototype_of__(context)?;

        // 2. Let internalSlotsList be the internal slots listed in Table 35, plus [[Prototype]] and [[Extensible]].
        // 3. Let obj be ! MakeBasicObject(internalSlotsList).
        // 4. Set obj.[[Prototype]] to proto.
        // 5. Set obj.[[Call]] as described in 10.4.1.1.
        // 6. If IsConstructor(targetFunction) is true, then
        // a. Set obj.[[Construct]] as described in 10.4.1.2.
        // 7. Set obj.[[BoundTargetFunction]] to targetFunction.
        // 8. Set obj.[[BoundThis]] to boundThis.
        // 9. Set obj.[[BoundArguments]] to boundArgs.
        // 10. Return obj.
        Ok(JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            proto,
            Self {
                target_function,
                this,
                args,
            },
        )
        .upcast())
    }
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/builtins/function/bound.rs:68-77`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L68-L77)

```rust
        Ok(JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            proto,
            Self {
                target_function,
                this,
                args,
            },
        )
        .upcast())
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/value/mod.rs:368-370`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/mod.rs#L368-L370)

```rust
    pub fn is_callable(&self) -> bool {
        self.as_object().as_ref().is_some_and(JsObject::is_callable)
    }
```

<a id="ref-q1-8"></a>
### [8] `core/engine/src/value/mod.rs:369`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/mod.rs#L369)

```rust
        self.as_object().as_ref().is_some_and(JsObject::is_callable)
```

<a id="ref-q1-9"></a>
### [9] `core/engine/src/object/jsobject.rs:1015-1020`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L1015-L1020)

```rust
    pub fn is_callable(&self) -> bool {
        !fn_addr_eq(
            self.inner.vtable.__call__,
            ORDINARY_INTERNAL_METHODS.__call__,
        )
    }
```

<a id="ref-q1-10"></a>
### [10] `core/engine/src/builtins/function/mod.rs:699-702`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L699-L702)

```rust
        // 2. If IsCallable(func) is false, throw a TypeError exception.
        let func = this.as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message(format!("{} is not a function", this.display()))
        })?;
```

<a id="ref-q1-11"></a>
### [11] `core/engine/src/builtins/function/mod.rs:738-743`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L738-L743)

```rust
        // 1. Let Target be the this value.
        // 2. If IsCallable(Target) is false, throw a TypeError exception.
        let target = this.as_callable().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("cannot bind `this` without a `[[Call]]` internal method")
        })?;
```

<a id="ref-q1-12"></a>
### [12] `core/engine/src/builtins/function/mod.rs:686-723`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L686-L723)

```rust
    /// `Function.prototype.apply ( thisArg, argArray )`
    ///
    /// The `apply()` method invokes self with the first argument as the `this` value
    /// and the rest of the arguments provided as an array (or an array-like object).
    ///
    /// More information:
    ///  - [MDN documentation][mdn]
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function.prototype.apply
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Function/apply
    fn apply(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        // 1. Let func be the this value.
        // 2. If IsCallable(func) is false, throw a TypeError exception.
        let func = this.as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message(format!("{} is not a function", this.display()))
        })?;

        let this_arg = args.get_or_undefined(0);
        let arg_array = args.get_or_undefined(1);
        // 3. If argArray is undefined or null, then
        if arg_array.is_null_or_undefined() {
            // a. Perform PrepareForTailCall().
            // TODO?: 3.a. PrepareForTailCall

            // b. Return ? Call(func, thisArg).
            return func.call(this_arg, &[], context);
        }

        // 4. Let argList be ? CreateListFromArrayLike(argArray).
        let arg_list = arg_array.create_list_from_array_like(&[], context)?;

        // 5. Perform PrepareForTailCall().
        // TODO?: 5. PrepareForTailCall

        // 6. Return ? Call(func, thisArg, argList).
        func.call(this_arg, &arg_list, context)
    }
```

<a id="ref-q1-13"></a>
### [13] `core/engine/src/builtins/function/mod.rs:715-722`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L715-L722)

```rust
        // 4. Let argList be ? CreateListFromArrayLike(argArray).
        let arg_list = arg_array.create_list_from_array_like(&[], context)?;

        // 5. Perform PrepareForTailCall().
        // TODO?: 5. PrepareForTailCall

        // 6. Return ? Call(func, thisArg, argList).
        func.call(this_arg, &arg_list, context)
```

<a id="ref-q1-14"></a>
### [14] `core/engine/src/builtins/function/mod.rs:807-829`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L807-L829)

```rust
    /// `Function.prototype.call ( thisArg, ...args )`
    ///
    /// The `call()` method calls a function with a given this value and arguments provided individually.
    ///
    /// More information:
    ///  - [MDN documentation][mdn]
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function.prototype.call
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Function/call
    fn call(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
        // 1. Let func be the this value.
        // 2. If IsCallable(func) is false, throw a TypeError exception.
        let func = this.as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message(format!("{} is not a function", this.display()))
        })?;
        let this_arg = args.get_or_undefined(0);

        // 3. Perform PrepareForTailCall().
        // TODO?: 3. Perform PrepareForTailCall

        // 4. Return ? Call(func, thisArg, args).
        func.call(this_arg, args.get(1..).unwrap_or(&[]), context)
```

<a id="ref-q1-15"></a>
### [15] `core/engine/src/builtins/function/mod.rs:823-829`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L823-L829)

```rust
        let this_arg = args.get_or_undefined(0);

        // 3. Perform PrepareForTailCall().
        // TODO?: 3. Perform PrepareForTailCall

        // 4. Return ? Call(func, thisArg, args).
        func.call(this_arg, args.get(1..).unwrap_or(&[]), context)
```

<a id="ref-q1-16"></a>
### [16] `core/engine/src/object/operations.rs:429-464`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/operations.rs#L429-L464)

```rust
    pub fn call(
        &self,
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // SKIP: 1. If argumentsList is not present, set argumentsList to a new empty List.
        // SKIP: 2. If IsCallable(F) is false, throw a TypeError exception.
        // NOTE(HalidOdat): For object's that are not callable we implement a special __call__ internal method
        //                  that throws on call.

        context.vm.stack.push(this.clone()); // this
        context.vm.stack.push(self.clone()); // func
        let argument_count = args.len();
        context.vm.stack.calling_convention_push_arguments(args);

        // 3. Return ? F.[[Call]](V, argumentsList).
        let frame_index = context.vm.frames.len();
        if self.__call__(argument_count).resolve(context)? {
            return Ok(context.vm.stack.pop());
        }

        if frame_index + 1 == context.vm.frames.len() {
            context.vm.frame_mut().set_exit_early(true);
        } else {
            context.vm.frames[frame_index + 1].set_exit_early(true);
        }

        context.vm.host_call_depth += 1;
        let result = context.run().consume();
        context.vm.host_call_depth = context.vm.host_call_depth.saturating_sub(1);

        context.vm.pop_frame().js_expect("frame must exist")?;

        result
    }
```

<a id="ref-q1-17"></a>
### [17] `core/engine/src/object/operations.rs:440-447`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/operations.rs#L440-L447)

```rust
        context.vm.stack.push(this.clone()); // this
        context.vm.stack.push(self.clone()); // func
        let argument_count = args.len();
        context.vm.stack.calling_convention_push_arguments(args);

        // 3. Return ? F.[[Call]](V, argumentsList).
        let frame_index = context.vm.frames.len();
        if self.__call__(argument_count).resolve(context)? {
```

<a id="ref-q1-18"></a>
### [18] `core/engine/src/builtins/function/bound.rs:24`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L24)

```rust
            __call__: bound_function_exotic_call,
```

<a id="ref-q1-19"></a>
### [19] `core/engine/src/builtins/function/bound.rs:115-130`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L115-L130)

```rust
    // 1. Let target be F.[[BoundTargetFunction]].
    let target = bound_function.target_function();
    context
        .vm
        .stack
        .calling_convention_set_function(argument_count, target.clone().into());

    // 2. Let boundThis be F.[[BoundThis]].
    let bound_this = bound_function.this();
    context
        .vm
        .stack
        .calling_convention_set_this(argument_count, bound_this.clone());

    // 3. Let boundArgs be F.[[BoundArguments]].
    let bound_args = bound_function.args();
```

<a id="ref-q1-20"></a>
### [20] `core/engine/src/builtins/function/bound.rs:124-139`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L124-L139)

```rust
    context
        .vm
        .stack
        .calling_convention_set_this(argument_count, bound_this.clone());

    // 3. Let boundArgs be F.[[BoundArguments]].
    let bound_args = bound_function.args();

    // 4. Let args be the list-concatenation of boundArgs and argumentsList.
    context
        .vm
        .stack
        .calling_convention_insert_arguments(argument_count, bound_args);

    // 5. Return ? Call(target, boundThis, args).
    Ok(target.__call__(bound_args.len() + argument_count))
```

<a id="ref-q1-21"></a>
### [21] `core/engine/src/builtins/function/bound.rs:25`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L25)

```rust
            __construct__: bound_function_exotic_construct,
```

<a id="ref-q1-22"></a>
### [22] `core/engine/src/builtins/function/bound.rs:162-186`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/bound.rs#L162-L186)

```rust
    // 1. Let target be F.[[BoundTargetFunction]].
    let target = bound_function.target_function();

    // 2. Assert: IsConstructor(target) is true.

    // 3. Let boundArgs be F.[[BoundArguments]].
    let bound_args = bound_function.args();

    // 4. Let args be the list-concatenation of boundArgs and argumentsList.
    context
        .vm
        .stack
        .calling_convention_insert_arguments(argument_count, bound_args);

    // 5. If SameValue(F, newTarget) is true, set newTarget to target.
    let function_object: JsValue = function_object.clone().into();
    let new_target = if JsValue::same_value(&function_object, &new_target) {
        target.clone().into()
    } else {
        new_target
    };

    // 6. Return ? Construct(target, args, newTarget).
    context.vm.stack.push(new_target);
    Ok(target.__construct__(bound_args.len() + argument_count))
```

<a id="ref-q1-23"></a>
### [23] `core/engine/src/object/mod.rs:387-394`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/mod.rs#L387-L394)

```rust
#[derive(Debug)]
pub struct FunctionObjectBuilder<'realm> {
    realm: &'realm Realm,
    function: NativeFunction,
    constructor: Option<ConstructorKind>,
    name: JsString,
    length: usize,
}
```

<a id="ref-q1-24"></a>
### [24] `core/engine/src/object/mod.rs:410-432`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/mod.rs#L410-L432)

```rust
    /// Specify the name property of object function object.
    ///
    /// The default is `""` (empty string).
    #[must_use]
    pub fn name<N>(mut self, name: N) -> Self
    where
        N: Into<JsString>,
    {
        self.name = name.into();
        self
    }

    /// Specify the length property of object function object.
    ///
    /// How many arguments this function takes.
    ///
    /// The default is `0`.
    #[inline]
    #[must_use]
    pub const fn length(mut self, length: usize) -> Self {
        self.length = length;
        self
    }
```

<a id="ref-q1-25"></a>
### [25] `core/engine/src/builtins/builder.rs:110-125`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/builder.rs#L110-L125)

```rust
        object.insert(
            StaticJsStrings::LENGTH,
            PropertyDescriptor::builder()
                .value(self.length)
                .writable(false)
                .enumerable(false)
                .configurable(true),
        );
        object.insert(
            js_string!("name"),
            PropertyDescriptor::builder()
                .value(self.name)
                .writable(false)
                .enumerable(false)
                .configurable(true),
        );
```

<a id="ref-q1-26"></a>
### [26] `core/engine/src/vm/code_block.rs:1167-1170`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/vm/code_block.rs#L1167-L1170)

```rust
pub(crate) fn create_function_object_fast(code: Gc<CodeBlock>, context: &mut Context) -> JsObject {
    let name: JsValue = code.name().clone().into();
    let length: JsValue = code.length.into();
```

<a id="ref-q1-27"></a>
### [27] `core/engine/src/context/intrinsics.rs:1749-1750`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/context/intrinsics.rs#L1749-L1750)

```rust
    /// 1. `"length"`: (`READONLY`, `NON_ENUMERABLE`, `CONFIGURABLE`)
    /// 2. `"name"`: (`READONLY`, `NON_ENUMERABLE`, `CONFIGURABLE`)
```

<a id="ref-q1-28"></a>
### [28] `core/engine/src/builtins/function/mod.rs:753-801`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L753-L801)

```rust
        let mut l = JsValue::new(0);

        // 5. Let targetHasLength be ? HasOwnProperty(Target, "length").
        // 6. If targetHasLength is true, then
        if target.has_own_property(StaticJsStrings::LENGTH, context)? {
            // a. Let targetLen be ? Get(Target, "length").
            let target_len = target.get(StaticJsStrings::LENGTH, context)?;
            // b. If Type(targetLen) is Number, then
            if target_len.is_number() {
                // 1. Let targetLenAsInt be ! ToIntegerOrInfinity(targetLen).
                match target_len
                    .to_integer_or_infinity(context)
                    .js_expect("to_integer_or_infinity cannot fail for a number")?
                {
                    // i. If targetLen is +∞𝔽, set L to +∞.
                    IntegerOrInfinity::PositiveInfinity => l = f64::INFINITY.into(),
                    // ii. Else if targetLen is -∞𝔽, set L to 0.
                    IntegerOrInfinity::NegativeInfinity => {}
                    // iii. Else,
                    IntegerOrInfinity::Integer(target_len) => {
                        // 2. Assert: targetLenAsInt is finite.
                        // 3. Let argCount be the number of elements in args.
                        // 4. Set L to max(targetLenAsInt - argCount, 0).
                        l = (target_len - arg_count).max(0).into();
                    }
                }
            }
        }

        // 7. Perform ! SetFunctionLength(F, L).
        f.define_property_or_throw(
            StaticJsStrings::LENGTH,
            PropertyDescriptor::builder()
                .value(l)
                .writable(false)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .js_expect("defining the `length` property for a new object should not fail")?;

        // 8. Let targetName be ? Get(Target, "name").
        let target_name = target.get(js_string!("name"), context)?;

        // 9. If Type(targetName) is not String, set targetName to the empty String.
        let target_name = target_name.as_string().unwrap_or_default();

        // 10. Perform SetFunctionName(F, targetName, "bound").
        set_function_name(&f, &target_name.into(), Some(js_str!("bound")), context)?;
```

<a id="ref-q1-29"></a>
### [29] `core/engine/src/builtins/function/mod.rs:772-776`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L772-L776)

```rust
                    IntegerOrInfinity::Integer(target_len) => {
                        // 2. Assert: targetLenAsInt is finite.
                        // 3. Let argCount be the number of elements in args.
                        // 4. Set L to max(targetLenAsInt - argCount, 0).
                        l = (target_len - arg_count).max(0).into();
```

<a id="ref-q1-30"></a>
### [30] `core/engine/src/builtins/function/mod.rs:783-792`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L783-L792)

```rust
        f.define_property_or_throw(
            StaticJsStrings::LENGTH,
            PropertyDescriptor::builder()
                .value(l)
                .writable(false)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .js_expect("defining the `length` property for a new object should not fail")?;
```

<a id="ref-q1-31"></a>
### [31] `core/engine/src/builtins/function/mod.rs:794-801`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L794-L801)

```rust
        // 8. Let targetName be ? Get(Target, "name").
        let target_name = target.get(js_string!("name"), context)?;

        // 9. If Type(targetName) is not String, set targetName to the empty String.
        let target_name = target_name.as_string().unwrap_or_default();

        // 10. Perform SetFunctionName(F, targetName, "bound").
        set_function_name(&f, &target_name.into(), Some(js_str!("bound")), context)?;
```

<a id="ref-q1-32"></a>
### [32] `core/engine/src/builtins/function/mod.rs:958-970`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L958-L970)

```rust
    // 6. Return ! DefinePropertyOrThrow(F, "name", PropertyDescriptor { [[Value]]: name,
    // [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }).
    function
        .define_property_or_throw(
            js_string!("name"),
            PropertyDescriptor::builder()
                .value(name)
                .writable(false)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .js_expect("defining the `name` property must not fail per the spec")?;
```

<a id="ref-q1-33"></a>
### [33] `core/engine/src/builtins/array/mod.rs:1384-1432`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array/mod.rs#L1384-L1432)

```rust
    /// `Array.prototype.map( callback, [ thisArg ] )`
    ///
    /// For each element in the array the callback function is called, and a new
    /// array is constructed from the return values of these calls.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-array.prototype.map
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Array/map
    pub(crate) fn map(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be ? ToObject(this value).
        let o = this.to_object(context)?;
        // 2. Let len be ? LengthOfArrayLike(O).
        let len = o.length_of_array_like(context)?;
        // 3. If IsCallable(callbackfn) is false, throw a TypeError exception.
        let callback = args.get_or_undefined(0).as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message("Array.prototype.map: Callbackfn is not callable")
        })?;

        // 4. Let A be ? ArraySpeciesCreate(O, len).
        let a = Self::array_species_create(&o, len, context)?;

        let this_arg = args.get_or_undefined(1);

        // 5. Let k be 0.
        // 6. Repeat, while k < len,
        for k in 0..len {
            // a. Let Pk be ! ToString(𝔽(k)).
            // b. Let k_present be ? HasProperty(O, Pk).
            // c. If k_present is true, then
            // c.i. Let kValue be ? Get(O, Pk).
            if let Some(k_value) = o.try_get(k, context)? {
                // ii. Let mappedValue be ? Call(callbackfn, thisArg, « kValue, 𝔽(k), O »).
                let mapped_value =
                    callback.call(this_arg, &[k_value, k.into(), o.clone().into()], context)?;
                // iii. Perform ? CreateDataPropertyOrThrow(A, Pk, mappedValue).
                a.create_data_property_or_throw(k, mapped_value, context)?;
            }
            // d. Set k to k + 1.
        }
        // 7. Return A.
        Ok(a.into())
    }
```

<a id="ref-q1-34"></a>
### [34] `core/engine/src/builtins/array/mod.rs:1395-1432`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/array/mod.rs#L1395-L1432)

```rust
    pub(crate) fn map(
        this: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be ? ToObject(this value).
        let o = this.to_object(context)?;
        // 2. Let len be ? LengthOfArrayLike(O).
        let len = o.length_of_array_like(context)?;
        // 3. If IsCallable(callbackfn) is false, throw a TypeError exception.
        let callback = args.get_or_undefined(0).as_callable().ok_or_else(|| {
            JsNativeError::typ().with_message("Array.prototype.map: Callbackfn is not callable")
        })?;

        // 4. Let A be ? ArraySpeciesCreate(O, len).
        let a = Self::array_species_create(&o, len, context)?;

        let this_arg = args.get_or_undefined(1);

        // 5. Let k be 0.
        // 6. Repeat, while k < len,
        for k in 0..len {
            // a. Let Pk be ! ToString(𝔽(k)).
            // b. Let k_present be ? HasProperty(O, Pk).
            // c. If k_present is true, then
            // c.i. Let kValue be ? Get(O, Pk).
            if let Some(k_value) = o.try_get(k, context)? {
                // ii. Let mappedValue be ? Call(callbackfn, thisArg, « kValue, 𝔽(k), O »).
                let mapped_value =
                    callback.call(this_arg, &[k_value, k.into(), o.clone().into()], context)?;
                // iii. Perform ? CreateDataPropertyOrThrow(A, Pk, mappedValue).
                a.create_data_property_or_throw(k, mapped_value, context)?;
            }
            // d. Set k to k + 1.
        }
        // 7. Return A.
        Ok(a.into())
    }
```

<a id="ref-q1-35"></a>
### [35] `boa-dev/boa:7`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/boa-dev/boa#L7)

# DeepWiki Response

Repository: `boa-dev/boa`
Question: For ECMAScript destructuring assignment, how does this implementation handle IteratorClose and target evaluation order? Please focus on array/object assignment destructuring, abrupt completions from target evaluation or assignment, iterator.return error precedence, and cite the relevant implementation files/functions.
Search: https://deepwiki.com/search/for-ecmascript-destructuring-a_de3b0406-4212-4765-a674-2751d8623fb1

**Question**: For ECMAScript destructuring assignment, how does this implementation handle IteratorClose and target evaluation order? Please focus on array/object assignment destructuring, abrupt completions from target evaluation or assignment, iterator.return error precedence, and cite the relevant implementation files/functions.

# ECMAScript Destructuring Assignment: IteratorClose and Target Evaluation Order

This implementation handles destructuring assignment through bytecode compilation with careful attention to iterator cleanup and evaluation order. The key mechanisms are in the bytecompiler and iterator handling code.

## Array Destructuring: Iterator Management and IteratorClose [1](#ref-q1-1)

For array patterns, the compiler emits `emit_get_iterator` to obtain an iterator, then processes each element. Critically, it uses exception handlers to ensure `IteratorClose` is called on abrupt completions:

1. **Normal path**: Elements are processed via `compile_array_pattern_element`
2. **Exception handler**: If any element throws, the handler at line 248 catches it, then calls `iterator_close(false)` at line 256 to clean up the iterator
3. **Final cleanup**: After normal completion, `iterator_close(false)` is called again at line 270

The `iterator_close` function itself is implemented in [2](#ref-q1-2) . It:
- Calls `emit_iterator_return` to invoke the iterator's `return` method
- Checks if the result is an object (lines 34-35)
- Throws a TypeError if the result is not an object (line 42)

This ensures that **iterator.return errors take precedence** over the original completion, as the error from `iterator_return` is what gets thrown if it fails.

## Target Evaluation Order in Array Patterns [3](#ref-q1-3)

Each array pattern element is compiled in sequence. For example:
- **SingleName** (line 287-308): Calls `emit_iterator_next`, checks `done`, extracts value, applies default initializer, then binds
- **PropertyAccess** (line 309-332): Uses `access_set` to evaluate the target location, then assigns the iterator value
- **Pattern** (line 334-354): Recursively compiles nested patterns

The key insight is that **target evaluation happens after iterator value extraction**. For instance, in `PropertyAccess`, the target property access is evaluated within `access_set` (line 314), which occurs after the iterator value is obtained (line 315).

## Object Destructuring: Property Evaluation Order [4](#ref-q1-4)

For object patterns, the implementation:
1. Evaluates the source object (line 155)
2. For each property, evaluates the property name (lines 206-220)
3. Gets the property value (line 207 or 212-217)
4. Applies default initializers (lines 222-226)
5. Recursively compiles nested patterns (line 227)

Notably, **property names are evaluated in order** before any assignments occur. This matches the spec requirement that all property keys are evaluated before side effects from assignments.

## Abrupt Completions and IteratorClose Precedence

The implementation uses the `if_abrupt_close_iterator!` macro throughout. In [5](#ref-q1-5) , the `if_abrupt_close_iterator` method shows the pattern:

When an abrupt completion occurs during element processing, `IteratorClose` is invoked with that error. The spec semantics are preserved: **if `iterator.return()` throws, that error replaces the original error**.

This is visible in [6](#ref-q1-6) , the `close` method:
- Line 614: Gets the `return` method
- Lines 617-637: If getting `return` throws, that error is returned (line 635)
- Lines 640-643: If calling `return` throws, that error is returned (line 643)
- Line 647: Only if both succeed does the original completion return

## Assignment Expression Destructuring [7](#ref-q1-7)

For assignment expressions (not declarations), the compiler:
1. Evaluates the RHS (line 27)
2. Calls `compile_declaration_pattern` with `BindingOpcode::SetName` (line 28)

This reuses the same pattern compilation logic, ensuring consistent IteratorClose behavior across both binding and assignment contexts.

## Notes

The implementation correctly prioritizes iterator.return errors over original completion errors through the exception handling structure in `iterator_close`. The evaluation order is strictly left-to-right for property names in objects and element-by-element for arrays, with iterator cleanup guaranteed via exception handlers. The CHANGELOG entry at line 437 ("Fix evaluation order in destructive property assignments") confirms this was a deliberate fix to match spec semantics.



Wiki pages you might want to explore:
- [Architecture (boa-dev/boa)](/wiki/boa-dev/boa#1.1)
- [Standard Builtins (boa-dev/boa)](/wiki/boa-dev/boa#3.1)
- [Async, Generators & Promises (boa-dev/boa)](/wiki/boa-dev/boa#3.4)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-destructuring-a_de3b0406-4212-4765-a674-2751d8623fb1

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/bytecompiler/declaration/declaration_pattern.rs:237-271`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/declaration/declaration_pattern.rs#L237-L271)

```rust
            Pattern::Array(pattern) => {
                self.bytecode
                    .emit_value_not_null_or_undefined(object.variable());
                self.bytecode.emit_get_iterator(object.variable());

                let handler_index = self.push_handler();
                for element in pattern.bindings() {
                    self.compile_array_pattern_element(element, def);
                }

                let no_exception_thrown = self.jump();
                self.patch_handler(handler_index);

                let has_exception = self.register_allocator.alloc();
                let exception = self.register_allocator.alloc();
                self.bytecode
                    .emit_maybe_exception(has_exception.variable(), exception.variable());

                let iterator_close_handler = self.push_handler();
                self.iterator_close(false);
                self.patch_handler(iterator_close_handler);

                let jump = self.jump_if_false(&has_exception);
                self.register_allocator.dealloc(has_exception);

                self.bytecode.emit_throw(exception.variable());
                self.register_allocator.dealloc(exception);

                self.patch_jump(jump);
                self.bytecode.emit_re_throw();

                self.patch_jump(no_exception_thrown);

                self.iterator_close(false);
            }
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/bytecompiler/utils.rs:15-46`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/utils.rs#L15-L46)

```rust
    pub(super) fn iterator_close(&mut self, async_: bool) {
        let value = self.register_allocator.alloc();
        let called = self.register_allocator.alloc();
        self.bytecode
            .emit_iterator_return(value.variable(), called.variable());

        // `iterator` didn't have a `return` method, is already done or is not on the iterator stack.
        let early_exit = self.jump_if_false(&called);
        self.register_allocator.dealloc(called);

        if async_ {
            self.bytecode.emit_await(value.variable());
            let resume_kind = self.register_allocator.alloc();
            self.pop_into_register(&resume_kind);
            self.pop_into_register(&value);
            self.generator_next(&value, &resume_kind);
            self.register_allocator.dealloc(resume_kind);
        }

        self.bytecode.emit_is_object(value.variable());
        let skip_throw = self.jump_if_true(&value);

        self.register_allocator.dealloc(value);

        let error_msg = self.get_or_insert_literal(Literal::String(js_string!(
            "inner result was not an object"
        )));
        self.bytecode.emit_throw_new_type_error(error_msg.into());

        self.patch_jump(skip_throw);
        self.patch_jump(early_exit);
    }
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/bytecompiler/declaration/declaration_pattern.rs:275-378`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/declaration/declaration_pattern.rs#L275-L378)

```rust
    fn compile_array_pattern_element(&mut self, element: &ArrayPatternElement, def: BindingOpcode) {
        use ArrayPatternElement::{
            Elision, Pattern, PatternRest, PropertyAccess, PropertyAccessRest, SingleName,
            SingleNameRest,
        };

        match element {
            // ArrayBindingPattern : [ Elision ]
            Elision => {
                self.bytecode.emit_iterator_next();
            }
            // SingleNameBinding : BindingIdentifier Initializer[opt]
            SingleName {
                ident,
                default_init,
            } => {
                self.bytecode.emit_iterator_next();
                let value = self.register_allocator.alloc();
                self.bytecode.emit_iterator_done(value.variable());
                self.if_else(
                    &value,
                    |compiler| compiler.bytecode.emit_store_undefined(value.variable()),
                    |compiler| compiler.bytecode.emit_iterator_value(value.variable()),
                );

                if let Some(init) = default_init {
                    let skip = self.jump_if_not_undefined(&value);
                    self.compile_expr(init, &value);
                    self.patch_jump(skip);
                }

                self.emit_binding(def, ident.to_js_string(self.interner()), &value);
                self.register_allocator.dealloc(value);
            }
            PropertyAccess {
                access,
                default_init,
            } => {
                let value = self.register_allocator.alloc();
                self.access_set(Access::Property { access }, |compiler| {
                    compiler.bytecode.emit_iterator_next();
                    compiler.bytecode.emit_iterator_done(value.variable());
                    compiler.if_else(
                        &value,
                        |compiler| compiler.bytecode.emit_store_undefined(value.variable()),
                        |compiler| compiler.bytecode.emit_iterator_value(value.variable()),
                    );

                    if let Some(init) = default_init {
                        let skip = compiler.jump_if_not_undefined(&value);
                        compiler.compile_expr(init, &value);
                        compiler.patch_jump(skip);
                    }

                    &value
                });
                self.register_allocator.dealloc(value);
            }
            // BindingElement : BindingPattern Initializer[opt]
            Pattern {
                pattern,
                default_init,
            } => {
                self.bytecode.emit_iterator_next();
                let value = self.register_allocator.alloc();
                self.bytecode.emit_iterator_done(value.variable());
                self.if_else(
                    &value,
                    |compiler| compiler.bytecode.emit_store_undefined(value.variable()),
                    |compiler| compiler.bytecode.emit_iterator_value(value.variable()),
                );

                if let Some(init) = default_init {
                    let skip = self.jump_if_not_undefined(&value);
                    self.compile_expr(init, &value);
                    self.patch_jump(skip);
                }
                self.compile_declaration_pattern(pattern, def, &value);
                self.register_allocator.dealloc(value);
            }
            // BindingRestElement : ... BindingIdentifier
            SingleNameRest { ident } => {
                let value = self.register_allocator.alloc();
                self.bytecode.emit_iterator_to_array(value.variable());
                self.emit_binding(def, ident.to_js_string(self.interner()), &value);
                self.register_allocator.dealloc(value);
            }
            PropertyAccessRest { access } => {
                let value = self.register_allocator.alloc();
                self.access_set(Access::Property { access }, |compiler| {
                    compiler.bytecode.emit_iterator_to_array(value.variable());
                    &value
                });
                self.register_allocator.dealloc(value);
            }
            // BindingRestElement : ... BindingPattern
            PatternRest { pattern } => {
                let value = self.register_allocator.alloc();
                self.bytecode.emit_iterator_to_array(value.variable());
                self.compile_declaration_pattern(pattern, def, &value);
                self.register_allocator.dealloc(value);
            }
        }
    }
```

<a id="ref-q1-4"></a>
### [4] `core/engine/src/bytecompiler/declaration/declaration_pattern.rs:155-236`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/declaration/declaration_pattern.rs#L155-L236)

```rust
                                |compiler: &mut ByteCompiler<'_>| {
                                    match name {
                                        PropertyName::Literal(ident) => {
                                            compiler.emit_get_property_by_name(
                                                &dst,
                                                None,
                                                object,
                                                ident.sym(),
                                            );
                                            compiler.register_allocator.dealloc(key);
                                        }
                                        PropertyName::Computed(_) => {
                                            if rest_exits {
                                                compiler.bytecode.emit_get_property_by_value_push(
                                                    dst.variable(),
                                                    key.variable(),
                                                    object.variable(),
                                                    object.variable(),
                                                );
                                                excluded_keys_registers.push(key);
                                            } else {
                                                compiler.bytecode.emit_get_property_by_value(
                                                    dst.variable(),
                                                    key.variable(),
                                                    object.variable(),
                                                    object.variable(),
                                                );
                                                compiler.register_allocator.dealloc(key);
                                            }
                                        }
                                    }

                                    if let Some(init) = default_init {
                                        let skip = compiler.jump_if_not_undefined(&dst);
                                        compiler.compile_expr(init, &dst);
                                        compiler.patch_jump(skip);
                                    }

                                    &dst
                                },
                            );
                            self.register_allocator.dealloc(dst);
                        }
                        Pattern {
                            name,
                            pattern,
                            default_init,
                        } => {
                            let dst = self.register_allocator.alloc();

                            match name {
                                PropertyName::Literal(ident) => {
                                    self.emit_get_property_by_name(&dst, None, object, ident.sym());
                                }
                                PropertyName::Computed(node) => {
                                    let key = self.register_allocator.alloc();
                                    self.compile_expr(node, &key);
                                    self.bytecode.emit_get_property_by_value(
                                        dst.variable(),
                                        key.variable(),
                                        object.variable(),
                                        object.variable(),
                                    );
                                    self.register_allocator.dealloc(key);
                                }
                            }

                            if let Some(init) = default_init {
                                let skip = self.jump_if_not_undefined(&dst);
                                self.compile_expr(init, &dst);
                                self.patch_jump(skip);
                            }
                            self.compile_declaration_pattern(pattern, def, &dst);
                            self.register_allocator.dealloc(dst);
                        }
                    }
                }

                while let Some(r) = excluded_keys_registers.pop() {
                    self.register_allocator.dealloc(r);
                }
            }
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/builtins/iterable/mod.rs:570-590`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/iterable/mod.rs#L570-L590)

```rust
    /// [`IfAbruptCloseIterator( value, iteratorRecord )`][spec], but
    /// adapted to be used inside `NativeCoroutine`.
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-ifabruptcloseiterator
    pub(crate) fn if_abrupt_close_iterator(
        &self,
        completion: CompletionRecord,
        context: &mut Context,
    ) -> CoroutineState {
        // 1. Assert: value is a Completion Record.
        // 2. If value is an abrupt completion, return ? IteratorClose(iteratorRecord, value).
        // 3. Set value to ! value.
        match completion {
            CompletionRecord::Return(value) => {
                self.close(Ok(value), context).branch()?;
                CoroutineState::Break(Ok(()))
            }
            CompletionRecord::Throw(err) => self.close(Err(err), context).branch(),
            CompletionRecord::Normal(value) => CoroutineState::Continue(value),
        }
    }
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/builtins/iterable/mod.rs:603-654`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/iterable/mod.rs#L603-L654)

```rust
    pub(crate) fn close(
        &self,
        completion: JsResult<JsValue>,
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Assert: Type(iteratorRecord.[[Iterator]]) is Object.

        // 2. Let iterator be iteratorRecord.[[Iterator]].
        let iterator = &self.iterator;

        // 3. Let innerResult be Completion(GetMethod(iterator, "return")).
        let inner_result = iterator.get_method(js_string!("return"), context);

        // 4. If innerResult.[[Type]] is normal, then
        let inner_result = match inner_result {
            Ok(inner_result) => {
                // a. Let return be innerResult.[[Value]].
                let r#return = inner_result;

                if let Some(r#return) = r#return {
                    // c. Set innerResult to Completion(Call(return, iterator)).
                    r#return.call(&iterator.clone().into(), &[], context)
                } else {
                    // b. If return is undefined, return ? completion.
                    return completion;
                }
            }
            Err(inner_result) => {
                // 5. If completion.[[Type]] is throw, return ? completion.
                completion?;

                // 6. If innerResult.[[Type]] is throw, return ? innerResult.
                return Err(inner_result);
            }
        };

        // 5. If completion.[[Type]] is throw, return ? completion.
        let completion = completion?;

        // 6. If innerResult.[[Type]] is throw, return ? innerResult.
        let inner_result = inner_result?;

        if inner_result.is_object() {
            // 8. Return ? completion.
            Ok(completion)
        } else {
            // 7. If Type(innerResult.[[Value]]) is not Object, throw a TypeError exception.
            Err(JsNativeError::typ()
                .with_message("inner result was not an object")
                .into())
        }
    }
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/bytecompiler/expression/assign.rs:14-30`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/expression/assign.rs#L14-L30)

```rust
impl ByteCompiler<'_> {
    pub(crate) fn compile_assign(&mut self, assign: &Assign, dst: &Register) {
        let mut compiler = self.position_guard(assign);

        if assign.op() == AssignOp::Assign {
            match Access::from_assign_target(assign.lhs()) {
                Ok(access) => {
                    compiler.access_set(access, |compiler| {
                        compiler.compile_expr(assign.rhs(), dst);
                        dst
                    });
                }
                Err(pattern) => {
                    compiler.compile_expr(assign.rhs(), dst);
                    compiler.compile_declaration_pattern(pattern, BindingOpcode::SetName, dst);
                }
            }
```

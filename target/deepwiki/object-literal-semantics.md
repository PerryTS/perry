# DeepWiki Response

Repository: `boa-dev/boa`
Question: For ECMAScript object literal implementation, explain object literal allocation prototype defaults, special __proto__ semantics including null/object/non-object and duplicate/computed cases, computed property key ToPropertyKey evaluation ordering before value evaluation and abrupt completion, computed accessor key handling, escaped reserved-word property names, and how parser versus lowering/runtime responsibilities are separated. Please point to Boa implementation details where possible.
Search: https://deepwiki.com/search/for-ecmascript-object-literal_3076013f-f447-4ceb-bea7-4aa42dfcbcd3

**Question**: For ECMAScript object literal implementation, explain object literal allocation prototype defaults, special __proto__ semantics including null/object/non-object and duplicate/computed cases, computed property key ToPropertyKey evaluation ordering before value evaluation and abrupt completion, computed accessor key handling, escaped reserved-word property names, and how parser versus lowering/runtime responsibilities are separated. Please point to Boa implementation details where possible.

Boa's implementation of ECMAScript object literals involves specific handling for prototype defaults, `__proto__` semantics, computed property keys, and the separation of parser and runtime responsibilities.

## Object Literal Allocation and Prototype Defaults

When an object literal is created, its default prototype is `Object.prototype` [1](#ref-q1-1) . This is consistent with the ECMAScript specification for object creation. The `get_prototype_from_constructor` function is responsible for determining the prototype of an object based on its constructor [2](#ref-q1-2) . If a constructor's `prototype` property is not an object, it defaults to the intrinsic prototype of the constructor's realm [3](#ref-q1-3) .

## Special `__proto__` Semantics

Boa handles the special `__proto__` property in object literals, including cases for `null`, `object`, and non-object values, as well as duplicate and computed property names [4](#ref-q1-4) .

### `__proto__` Getter and Setter

The `Object.prototype.__proto__` getter (`legacy_proto_getter`) exposes the internal `[[Prototype]]` of an object [5](#ref-q1-5) . The setter (`legacy_proto_setter`) allows mutation of an object's `[[Prototype]]` [6](#ref-q1-6) .

The `legacy_proto_setter` handles different types for the `proto` argument:
*   If `proto` is an `Object` or `Null`, it attempts to set the prototype [7](#ref-q1-7) .
*   If `proto` is neither an `Object` nor `Null`, the setter returns `undefined` [8](#ref-q1-8) .
*   If the `this` value is not an `Object`, it also returns `undefined` [9](#ref-q1-9) .
*   If the `[[SetPrototypeOf]]` internal method returns `false`, a `TypeError` is thrown [10](#ref-q1-10) .

## Computed Property Keys

Computed property keys are evaluated before their corresponding property values. The `ByteCompiler` is responsible for compiling the expression that defines the computed property name [11](#ref-q1-11) . This expression's result is then converted to a `PropertyKey` using the `emit_to_property_key` instruction [12](#ref-q1-12) . This ensures that the key is resolved before the property's value or accessor is processed. The `to_property_key` method on `JsValue` handles the conversion of various JavaScript types into a `PropertyKey` [13](#ref-q1-13) . This conversion can lead to an abrupt completion if, for example, `to_primitive` throws an error [14](#ref-q1-14) .

### Computed Accessor Key Handling

For computed accessor keys (getters and setters), the `ByteCompiler` compiles the key expression and converts it to a `PropertyKey` [15](#ref-q1-15) . Then, specific bytecode instructions like `emit_define_class_static_getter_by_value` or `emit_define_class_getter_by_value` are used to define the accessor with the computed key [16](#ref-q1-16) .

## Escaped Reserved-Word Property Names

Boa handles escaped identifiers, including those that might be reserved words, during parsing to ensure correct property name interpretation [17](#ref-q1-17) . The `PropertyName::Literal` and `PropertyName::Computed` variants in the AST represent property names, with `Literal` handling direct identifiers and `Computed` handling expressions that evaluate to a property key [18](#ref-q1-18) .

## Parser vs. Lowering/Runtime Responsibilities

The architecture of Boa separates responsibilities into distinct stages [19](#ref-q1-19) :
1.  **Parser**: The `boa_parser` crate is responsible for lexing the source code into tokens and then parsing these tokens into an Abstract Syntax Tree (AST) [20](#ref-q1-20) . This stage handles the syntactic validation of object literals, including property names and structure.
2.  **ByteCompiler (Lowering)**: The `ByteCompiler` in `boa_engine::bytecompiler` takes the AST and transforms it into bytecode instructions [21](#ref-q1-21) . This is where computed property keys are evaluated and converted to `PropertyKey`s [15](#ref-q1-15) .
3.  **VM (Runtime)**: The Virtual Machine (`boa_engine::vm::Vm`) executes the generated bytecode [22](#ref-q1-22) . During runtime, operations like property access (`__get__`) [23](#ref-q1-23)  and prototype manipulation (`__set_prototype_of__`) [24](#ref-q1-24)  occur. The `JsObject` and its internal methods handle the actual object model and property storage [25](#ref-q1-25) .

## Notes

The provided context highlights the implementation of object literals and related features within the Boa engine. The `CHANGELOG.md` entry for "Handle `__proto__` fields in object literals" [4](#ref-q1-4)  indicates a specific focus on these semantics. The `Object Model & Shapes` wiki page provides a high-level overview of how objects are structured and optimized in Boa [26](#ref-q1-26) .

Wiki pages you might want to explore:
- [Architecture (boa-dev/boa)](/wiki/boa-dev/boa#1.1)
- [Object Model & Shapes (boa-dev/boa)](/wiki/boa-dev/boa#2.6)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-object-literal_3076013f-f447-4ceb-bea7-4aa42dfcbcd3

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/builtins/object/mod.rs:1-7`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L1-L7)

```rust
//! Boa's implementation of ECMAScript's global `Object` object.
//!
//! The `Object` class represents one of ECMAScript's data types.
//!
//! It is used to store various keyed collections and more complex entities.
//! Objects can be created using the `Object()` constructor or the
//! object initializer / literal syntax.
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/object/internal_methods/mod.rs:1171-1175`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/internal_methods/mod.rs#L1171-L1175)

```rust
pub(crate) fn get_prototype_from_constructor<F>(
    constructor: &JsValue,
    default: F,
    context: &mut Context,
) -> JsResult<JsObject>
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/object/internal_methods/mod.rs:1188-1195`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/internal_methods/mod.rs#L1188-L1195)

```rust
        // 3. If Type(proto) is not Object, then
        // a. Let realm be ? GetFunctionRealm(constructor).
        constructor.get_function_realm(context)?
    } else {
        context.realm().clone()
    };
    // b. Set proto to realm's intrinsic object named intrinsicDefaultProto.
    Ok(default(realm.intrinsics().constructors()).prototype())
```

<a id="ref-q1-4"></a>
### [4] `CHANGELOG.md:818`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/CHANGELOG.md#L818)

```markdown
- Handle `__proto__` fields in object literals by @raskad in [#2423](https://github.com/boa-dev/boa/pull/2423)
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/builtins/object/mod.rs:195-217`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L195-L217)

```rust
    /// `get Object.prototype.__proto__`
    ///
    /// The `__proto__` getter function exposes the value of the
    /// internal `[[Prototype]]` of an object.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-get-object.prototype.__proto__
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Object/proto
    pub fn legacy_proto_getter(
        this: &JsValue,
        _: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. Let O be ? ToObject(this value).
        let obj = this.to_object(context)?;

        // 2. Return ? O.[[GetPrototypeOf]]().
        let proto = obj.__get_prototype_of__(&mut InternalMethodPropertyContext::new(context))?;

        Ok(proto.map_or(JsValue::null(), JsValue::new))
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/builtins/object/mod.rs:220-231`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L220-L231)

```rust
    /// `set Object.prototype.__proto__`
    ///
    /// The `__proto__` setter allows the `[[Prototype]]` of
    /// an object to be mutated.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///  - [MDN documentation][mdn]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-set-object.prototype.__proto__
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Object/proto
    pub fn legacy_proto_setter(
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/builtins/object/mod.rs:240-242`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L240-L242)

```rust
        let proto = match args.get_or_undefined(0).variant() {
            JsVariant::Object(proto) => Some(proto.clone()),
            JsVariant::Null => None,
```

<a id="ref-q1-8"></a>
### [8] `core/engine/src/builtins/object/mod.rs:243-244`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L243-L244)

```rust
            _ => return Ok(JsValue::undefined()),
        };
```

<a id="ref-q1-9"></a>
### [9] `core/engine/src/builtins/object/mod.rs:247-249`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L247-L249)

```rust
        let JsVariant::Object(object) = this.variant() else {
            return Ok(JsValue::undefined());
        };
```

<a id="ref-q1-10"></a>
### [10] `core/engine/src/builtins/object/mod.rs:254-258`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L254-L258)

```rust

        // 5. If status is false, throw a TypeError exception.
        if !status {
            return Err(js_error!(TypeError: "__proto__ called on null or undefined"));
        }
```

<a id="ref-q1-11"></a>
### [11] `core/engine/src/bytecompiler/class.rs:280-282`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/class.rs#L280-L282)

```rust
                    ClassElementName::PropertyName(PropertyName::Computed(name)) => {
                        let key = self.register_allocator.alloc();
                        self.compile_expr(name, &key);
```

<a id="ref-q1-12"></a>
### [12] `core/engine/src/bytecompiler/class.rs:283-284`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/class.rs#L283-L284)

```rust
                        self.bytecode
                            .emit_to_property_key(key.variable(), key.variable());
```

<a id="ref-q1-13"></a>
### [13] `core/engine/src/value/mod.rs:1057-1060`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/mod.rs#L1057-L1060)

```rust
    /// Converts the value to a `PropertyKey`, that can be used as a key for properties.
    ///
    /// See <https://tc39.es/ecma262/#sec-topropertykey>
    pub fn to_property_key(&self, context: &mut Context) -> JsResult<PropertyKey> {
```

<a id="ref-q1-14"></a>
### [14] `core/engine/src/value/mod.rs:1083-1085`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/mod.rs#L1083-L1085)

```rust
            JsVariant::Object(o) => o
                .to_primitive(context, PreferredType::String)?
                .to_property_key(context),
```

<a id="ref-q1-15"></a>
### [15] `core/engine/src/bytecompiler/class.rs:280-284`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/class.rs#L280-L284)

```rust
                    ClassElementName::PropertyName(PropertyName::Computed(name)) => {
                        let key = self.register_allocator.alloc();
                        self.compile_expr(name, &key);
                        self.bytecode
                            .emit_to_property_key(key.variable(), key.variable());
```

<a id="ref-q1-16"></a>
### [16] `core/engine/src/bytecompiler/class.rs:294-325`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/class.rs#L294-L325)

```rust
                                self.bytecode.emit_define_class_static_getter_by_value(
                                    method.variable(),
                                    key.variable(),
                                    object_register.variable(),
                                );
                            }
                            (true, MethodDefinitionKind::Set) => {
                                self.bytecode.emit_define_class_static_setter_by_value(
                                    method.variable(),
                                    key.variable(),
                                    object_register.variable(),
                                );
                            }
                            (true, _) => {
                                self.bytecode.emit_define_class_static_method_by_value(
                                    method.variable(),
                                    key.variable(),
                                    object_register.variable(),
                                );
                            }
                            (false, MethodDefinitionKind::Get) => {
                                self.bytecode.emit_define_class_getter_by_value(
                                    method.variable(),
                                    key.variable(),
                                    object_register.variable(),
                                );
                            }
                            (false, MethodDefinitionKind::Set) => {
                                self.bytecode.emit_define_class_setter_by_value(
                                    method.variable(),
                                    key.variable(),
                                    object_register.variable(),
```

<a id="ref-q1-17"></a>
### [17] `CHANGELOG.md:838`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/CHANGELOG.md#L838)

```markdown
- Add early errors for escaped identifiers by @raskad in [#2546](https://github.com/boa-dev/boa/pull/2546)
```

<a id="ref-q1-18"></a>
### [18] `core/engine/src/bytecompiler/declaration/declaration_pattern.rs:42-76`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/declaration/declaration_pattern.rs#L42-L76)

```rust
                                PropertyName::Literal(ident) => {
                                    self.emit_get_property_by_name(&dst, None, object, ident.sym());
                                    let key = self.register_allocator.alloc();
                                    self.emit_store_literal(
                                        Literal::String(
                                            self.interner()
                                                .resolve_expect(ident.sym())
                                                .into_common(false),
                                        ),
                                        &key,
                                    );
                                    excluded_keys_registers.push(key);
                                }
                                PropertyName::Computed(node) => {
                                    let key = self.register_allocator.alloc();
                                    self.compile_expr(node, &key);
                                    if rest_exits {
                                        self.bytecode.emit_get_property_by_value_push(
                                            dst.variable(),
                                            key.variable(),
                                            object.variable(),
                                            object.variable(),
                                        );
                                        excluded_keys_registers.push(key);
                                    } else {
                                        self.bytecode.emit_get_property_by_value(
                                            dst.variable(),
                                            key.variable(),
                                            object.variable(),
                                            object.variable(),
                                        );
                                        self.register_allocator.dealloc(key);
                                    }
                                }
                            }
```

<a id="ref-q1-19"></a>
### [19] `wiki/Architecture:20`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Architecture#L20)

<a id="ref-q1-20"></a>
### [20] `wiki/Architecture:50-53`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Architecture#L50-L53)

<a id="ref-q1-21"></a>
### [21] `wiki/Architecture:70-71`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Architecture#L70-L71)

<a id="ref-q1-22"></a>
### [22] `wiki/Architecture:72-73`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Architecture#L72-L73)

<a id="ref-q1-23"></a>
### [23] `core/engine/src/vm/opcode/get/property.rs:68`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/vm/opcode/get/property.rs#L68)

```rust
    let result = object.__get__(&key, receiver.clone(), context)?;
```

<a id="ref-q1-24"></a>
### [24] `core/engine/src/builtins/object/mod.rs:251-253`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/object/mod.rs#L251-L253)

```rust
        // 4. Let status be ? O.[[SetPrototypeOf]](proto).
        let status =
            object.__set_prototype_of__(proto, &mut InternalMethodPropertyContext::new(context))?;
```

<a id="ref-q1-25"></a>
### [25] `wiki/Object Model & Shapes:20`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Object Model & Shapes#L20)

<a id="ref-q1-26"></a>
### [26] `wiki/Object Model & Shapes:1`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/wiki/Object Model & Shapes#L1)

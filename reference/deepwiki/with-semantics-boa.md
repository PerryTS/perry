# DeepWiki Response

Repository: `boa-dev/boa`
Question: How does this ECMAScript engine implement with statement object environment records? Please focus on ObjectEnvironmentRecord/withEnvironment HasBinding, GetBindingValue, SetMutableBinding, DeleteBinding, @@unscopables handling, closure capture through with object environments, and strict-mode syntax restrictions. Include concrete source references.
Search: https://deepwiki.com/search/how-does-this-ecmascript-engin_efd7bd82-9a12-486b-82a9-e5e76c71a4c0

**Question**: How does this ECMAScript engine implement with statement object environment records? Please focus on ObjectEnvironmentRecord/withEnvironment HasBinding, GetBindingValue, SetMutableBinding, DeleteBinding, @@unscopables handling, closure capture through with object environments, and strict-mode syntax restrictions. Include concrete source references.

The Boa ECMAScript engine implements `with` statement object environment records through the `Environment::Object` enum variant within its `EnvironmentStack` structure [1](#ref-q1-1) . This `EnvironmentStack` manages the lexical and variable scopes during execution [2](#ref-q1-2) .

## ObjectEnvironmentRecord Operations

### HasBinding
The `has_property` method of the `JsObject` associated with the `Object` environment is used to determine if a binding exists [3](#ref-q1-3) . This check is performed within the `Context::find_runtime_binding` and `Context::this_from_object_environment_binding` methods [4](#ref-q1-4)  [5](#ref-q1-5) .

### GetBindingValue
To retrieve the value of a binding, the `get` method of the underlying `JsObject` is called [6](#ref-q1-6) . This occurs in the `Context::get_binding` method [7](#ref-q1-7) .

### SetMutableBinding
Setting a mutable binding in an `Object` environment is handled by the `set` method of the `JsObject` [8](#ref-q1-8) . The `Context::set_binding` method orchestrates this operation, passing along the `strict` mode flag [9](#ref-q1-9) .

### DeleteBinding
The `__delete__` method of the `JsObject` is invoked to delete a binding from an `Object` environment [10](#ref-q1-10) . This is part of the `Context::delete_binding` implementation [11](#ref-q1-11) .

### @@unscopables Handling
The `@@unscopables` symbol is checked during binding lookups within `Object` environments [12](#ref-q1-12) . If an object has an `@@unscopables` property and it contains the binding's key with a `true` value, the binding is effectively "skipped" and the search continues up the environment chain [12](#ref-q1-12) . This logic is present in `Context::find_runtime_binding` and `Context::this_from_object_environment_binding` [12](#ref-q1-12)  [13](#ref-q1-13) .

## Closure Capture through `with` Object Environments
The `EnvironmentStack` can be snapshotted for closures using `EnvironmentStack::snapshot_for_closure`, which performs a cheap clone due to the linked-list implementation [14](#ref-q1-14) . This ensures that the closure retains access to the environment chain, including any `Object` environments, at the time of its creation.

## Strict-Mode Syntax Restrictions
The `with` statement is explicitly disallowed in strict mode code [15](#ref-q1-15) . The engine enforces this restriction, throwing a `SyntaxError` if a `with` statement is encountered in strict mode [16](#ref-q1-16) . This check is performed during parsing or compilation. The `ByteCompiler` also tracks whether it is currently compiling code within a `with` statement via the `in_with` field [17](#ref-q1-17) . Additionally, the `eval_declaration_instantiation_scope` function notes that `with` statements cannot contain any lexical declarations, thus avoiding var/let hoisting conflicts [18](#ref-q1-18) .

## Notes
The `EnvironmentStack` uses a linked-list like structure where each `EnvironmentNode` contains an `Environment` and a pointer to its `parent` [19](#ref-q1-19) . The `Environment` enum can be either `Declarative` or `Object` [1](#ref-q1-1) . The `push_object` method is responsible for adding an `Object` environment to the stack [20](#ref-q1-20) . The `PushObjectEnvironment` opcode triggers this action during VM execution [21](#ref-q1-21) .

Wiki pages you might want to explore:
- [Context & Execution Environment (boa-dev/boa)](/wiki/boa-dev/boa#2.3)
- [Parser & AST (boa-dev/boa)](/wiki/boa-dev/boa#5.4)

View this search on DeepWiki: https://deepwiki.com/search/how-does-this-ecmascript-engin_efd7bd82-9a12-486b-82a9-e5e76c71a4c0

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/environments/runtime/mod.rs:62-66`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L62-L66)

```rust
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) enum Environment {
    Declarative(Gc<DeclarativeEnvironment>),
    Object(JsObject),
}
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/environments/mod.rs:1-3`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/mod.rs#L1-L3)

```rust
//! Boa's implementation of ECMAScript's `Environment Records`.
//!
//! Environments contain the bindings of identifiers to their values.
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/environments/runtime/mod.rs:495`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L495)

```rust
                    if o.has_property(key.clone(), self)? {
```

<a id="ref-q1-4"></a>
### [4] `core/engine/src/environments/runtime/mod.rs:462`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L462)

```rust
    pub(crate) fn find_runtime_binding(&mut self, locator: &mut BindingLocator) -> JsResult<()> {
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/environments/runtime/mod.rs:521-523`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L521-L523)

```rust
        &mut self,
        locator: &BindingLocator,
    ) -> JsResult<Option<JsObject>> {
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/environments/runtime/mod.rs:617`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L617)

```rust
                    obj.get(key, self).map(Some)
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/environments/runtime/mod.rs:601`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L601)

```rust
    pub(crate) fn get_binding(&mut self, locator: &BindingLocator) -> JsResult<Option<JsValue>> {
```

<a id="ref-q1-8"></a>
### [8] `core/engine/src/environments/runtime/mod.rs:652`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L652)

```rust
                    obj.set(key, value, strict, self)?;
```

<a id="ref-q1-9"></a>
### [9] `core/engine/src/environments/runtime/mod.rs:629-634`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L629-L634)

```rust
    pub(crate) fn set_binding(
        &mut self,
        locator: &BindingLocator,
        value: JsValue,
        strict: bool,
    ) -> JsResult<()> {
```

<a id="ref-q1-10"></a>
### [10] `core/engine/src/environments/runtime/mod.rs:678`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L678)

```rust
                    let obj = obj.clone();
```

<a id="ref-q1-11"></a>
### [11] `core/engine/src/environments/runtime/mod.rs:666`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L666)

```rust
    pub(crate) fn delete_binding(&mut self, locator: &BindingLocator) -> JsResult<bool> {
```

<a id="ref-q1-12"></a>
### [12] `core/engine/src/environments/runtime/mod.rs:496-500`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L496-L500)

```rust
                        if let Some(unscopables) = o.get(JsSymbol::unscopables(), self)?.as_object()
                            && unscopables.get(key.clone(), self)?.to_boolean()
                        {
                            continue;
                        }
```

<a id="ref-q1-13"></a>
### [13] `core/engine/src/environments/runtime/mod.rs:553-557`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L553-L557)

```rust
                    if o.has_property(key.clone(), self)? {
                        if let Some(unscopables) = o.get(JsSymbol::unscopables(), self)?.as_object()
                            && unscopables.get(key.clone(), self)?.to_boolean()
                        {
                            continue;
```

<a id="ref-q1-14"></a>
### [14] `core/engine/src/environments/runtime/mod.rs:397-402`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L397-L402)

```rust
    ///
    /// With the linked-list implementation, this is just a clone since
    /// cloning is O(1) — a single ref-count bump on the tip pointer.
    pub(crate) fn snapshot_for_closure(&self) -> EnvironmentStack {
        self.clone()
    }
```

<a id="ref-q1-15"></a>
### [15] `core/engine/src/tests/mod.rs:404-406`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/tests/mod.rs#L404-L406)

```rust
fn strict_mode_with() {
    // Checks as per https://tc39.es/ecma262/#sec-with-statement-static-semantics-early-errors
    // that a with statement is an error in strict mode code.
```

<a id="ref-q1-16"></a>
### [16] `core/engine/src/tests/mod.rs:416-418`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/tests/mod.rs#L416-L418)

```rust
        JsNativeErrorKind::Syntax,
        "with statement not allowed in strict mode at line 3, col 5",
    )]);
```

<a id="ref-q1-17"></a>
### [17] `core/engine/src/bytecompiler/mod.rs:539-540`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/mod.rs#L539-L540)

```rust
    /// Whether the function is in a `with` statement.
    pub(crate) in_with: bool,
```

<a id="ref-q1-18"></a>
### [18] `core/ast/src/scope_analyzer.rs:2347-2348`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/ast/src/scope_analyzer.rs#L2347-L2348)

```rust
            // 1. NOTE: The environment of with statements cannot contain any lexical
            //    declaration so it doesn't need to be checked for var/let hoisting conflicts.
```

<a id="ref-q1-19"></a>
### [19] `core/engine/src/environments/runtime/mod.rs:408-411`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L408-L411)

```rust
        self.tip = Some(Gc::new(EnvironmentNode {
            env,
            parent: self.tip.take(),
        }));
```

<a id="ref-q1-20"></a>
### [20] `core/engine/src/environments/runtime/mod.rs:205-208`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/environments/runtime/mod.rs#L205-L208)

```rust
    /// Push a new object environment on the environments stack.
    pub(crate) fn push_object(&mut self, object: JsObject) {
        self.push_env(Environment::Object(object));
    }
```

<a id="ref-q1-21"></a>
### [21] `core/engine/src/vm/opcode/push/environment.rs:44-47`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/vm/opcode/push/environment.rs#L44-L47)

```rust
    pub(crate) fn operation(value: RegisterOperand, context: &mut Context) -> JsResult<()> {
        let object = context.vm.get_register(value.into()).clone();
        let object = object.to_object(context)?;
        context.vm.frame_mut().environments.push_object(object);
```

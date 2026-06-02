# DeepWiki Response

Repository: `boa-dev/boa`
Question: For ECMAScript AdditionExpression evaluation, explain the implementation flow for evaluating left and right operands, GetValue on references, assignment expression result values like (y = 1), ToPrimitive for addition, ToString for objects/functions, ordinary object default stringification, function default stringification, and observable evaluation order. Please cite the relevant files/functions.
Search: https://deepwiki.com/search/for-ecmascript-additionexpress_6b8788c1-e7f7-4706-994c-3ada161ab0af

**Question**: For ECMAScript AdditionExpression evaluation, explain the implementation flow for evaluating left and right operands, GetValue on references, assignment expression result values like (y = 1), ToPrimitive for addition, ToString for objects/functions, ordinary object default stringification, function default stringification, and observable evaluation order. Please cite the relevant files/functions.

The user is asking about the implementation details of ECMAScript AdditionExpression evaluation within the Boa JavaScript engine. This includes how operands are evaluated, `GetValue` on references, assignment expression result values, `ToPrimitive` conversion for addition, `ToString` for objects and functions, default stringification for ordinary objects and functions, and observable evaluation order.

The core of addition expression evaluation is handled by the `add` method of the `JsValue` type [1](#ref-q1-1) . This method implements the ECMAScript specification for the binary `+` operator.

### Evaluation of Left and Right Operands

The `add` method first attempts a "fast path" for numeric and string additions based on the `JsVariant` of the operands [2](#ref-q1-2) . If the operands are both `Integer32`, `Float64`, or `BigInt`, it performs the corresponding arithmetic operation [3](#ref-q1-3) . If both are `String`, it performs string concatenation [4](#ref-q1-4) .

If the fast path is not taken, it proceeds to the "slow path" which involves `ToPrimitive` conversions [5](#ref-q1-5) .

### GetValue on References and Assignment Expression Result Values

While the provided snippets do not directly show the `GetValue` operation on references, the `ByteCompiler` is responsible for compiling expressions, including assignments. The `compile_assign` method in `core/engine/src/bytecompiler/expression/assign.rs` handles assignment expressions [6](#ref-q1-6) . For a simple assignment (`AssignOp::Assign`), it compiles the right-hand side expression and then sets the value to the left-hand side [7](#ref-q1-7) . The result of an assignment expression is the value of the right-hand side after assignment.

### ToPrimitive for Addition

For the "slow path" of addition, both operands are converted to primitive values using `to_primitive` with `PreferredType::Default` [8](#ref-q1-8) .

The `to_primitive` method of `JsObject` handles the conversion of an object to a primitive value [9](#ref-q1-9) . It first checks for a `[Symbol.toPrimitive]` method on the object [10](#ref-q1-10) . If present, it calls this method with a hint ("default", "string", or "number") [11](#ref-q1-11) . If the `[Symbol.toPrimitive]` method returns an object, a `TypeError` is thrown [12](#ref-q1-12) .

If `[Symbol.toPrimitive]` is not found, `ordinary_to_primitive` is called [13](#ref-q1-13) . This method attempts to call `toString()` and `valueOf()` methods on the object in a specific order based on the `PreferredType` hint [14](#ref-q1-14) . For `PreferredType::String`, it tries `toString` then `valueOf`. For `PreferredType::Number` (or `Default`), it tries `valueOf` then `toString` [15](#ref-q1-15) . If a callable method returns a non-object primitive, that value is returned [16](#ref-q1-16) . If no primitive is obtained, a `TypeError` is thrown [17](#ref-q1-17) . A recursion limiter is also in place to prevent stack overflows for recursive objects during `ordinary_to_primitive` [18](#ref-q1-18) .

After `ToPrimitive` conversion, if either result is a `String`, the values are converted to strings and concatenated [19](#ref-q1-19) . Otherwise, they are converted to `Numeric` values and added [20](#ref-q1-20) .

### ToString for Objects/Functions

The `to_string` method on `JsValue` is used to convert a value to its string representation [21](#ref-q1-21) .

#### Ordinary Object Default Stringification

For ordinary objects, the `ordinary_to_primitive` method, when called with `PreferredType::String`, will attempt to call the object's `toString` method first [22](#ref-q1-22) . The default `Object.prototype.toString()` typically returns `"[object Object]"`.

#### Function Default Stringification

The `Function.prototype.toString()` implementation is found in `core/engine/src/builtins/function/mod.rs` [23](#ref-q1-23) . For built-in functions or native functions, it returns an implementation-defined string like `function name() { [native code] }` [24](#ref-q1-24) . For user-defined functions, it attempts to retrieve the source text from the code block [25](#ref-q1-25) .

### Observable Evaluation Order

ECMAScript specifies left-to-right evaluation of expressions [26](#ref-q1-26) . This is explicitly handled in operations like `abstract_relation` where a `left_first` flag determines the order of `to_primitive` calls to preserve this observable order [27](#ref-q1-27) . The `ByteCompiler` also processes expressions in the order they appear in the AST, ensuring left-to-right evaluation. For binary arithmetic operations, the `compile_binary_arithmetic` method compiles the right-hand side expression after the left-hand side has been processed implicitly by the `lhs` parameter [28](#ref-q1-28) .

## Notes

The `Architecture` wiki page provides a high-level overview of the Boa engine's components, including the `ByteCompiler` and `VM` [29](#ref-q1-29) . The `JavaScript Values` wiki page details the `JsValue` representation and its conversion mechanisms, which are fundamental to understanding `ToPrimitive` and `ToString` operations [30](#ref-q1-30) .

Wiki pages you might want to explore:
- [Architecture (boa-dev/boa)](/wiki/boa-dev/boa#1.1)
- [JavaScript Values (boa-dev/boa)](/wiki/boa-dev/boa#2.1)

View this search on DeepWiki: https://deepwiki.com/search/for-ecmascript-additionexpress_6b8788c1-e7f7-4706-994c-3ada161ab0af

## References

<a id="ref-q1-1"></a>
### [1] `core/engine/src/value/operations.rs:14-52`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L14-L52)

```rust
    pub fn add(&self, other: &Self, context: &mut Context) -> JsResult<Self> {
        Ok(match (self.variant(), other.variant()) {
            // Fast path:
            // Numeric add
            (JsVariant::Integer32(x), JsVariant::Integer32(y)) => x
                .checked_add(y)
                .map_or_else(|| Self::new(f64::from(x) + f64::from(y)), Self::new),
            (JsVariant::Float64(x), JsVariant::Float64(y)) => Self::new(x + y),
            (JsVariant::Integer32(x), JsVariant::Float64(y)) => Self::new(f64::from(x) + y),
            (JsVariant::Float64(x), JsVariant::Integer32(y)) => Self::new(x + f64::from(y)),
            (JsVariant::BigInt(x), JsVariant::BigInt(y)) => Self::new(JsBigInt::add(&x, &y)),

            // String concat
            (JsVariant::String(x), JsVariant::String(y)) => Self::from(js_string!(&x, &y)),

            // Slow path:
            (_, _) => {
                let x = self.to_primitive(context, PreferredType::Default)?;
                let y = other.to_primitive(context, PreferredType::Default)?;
                match (x.variant(), y.variant()) {
                    (JsVariant::String(x), _) => Self::from(js_string!(&x, &y.to_string(context)?)),
                    (_, JsVariant::String(y)) => Self::from(js_string!(&x.to_string(context)?, &y)),
                    (_, _) => {
                        match (x.to_numeric(context)?, y.to_numeric(context)?) {
                            (Numeric::Number(x), Numeric::Number(y)) => Self::new(x + y),
                            (Numeric::BigInt(ref x), Numeric::BigInt(ref y)) => {
                                Self::new(JsBigInt::add(x, y))
                            }
                            (_, _) => return Err(JsNativeError::typ()
                                .with_message(
                                    "cannot mix BigInt and other types, use explicit conversions",
                                )
                                .into()),
                        }
                    }
                }
            }
        })
    }
```

<a id="ref-q1-2"></a>
### [2] `core/engine/src/value/operations.rs:15-28`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L15-L28)

```rust
        Ok(match (self.variant(), other.variant()) {
            // Fast path:
            // Numeric add
            (JsVariant::Integer32(x), JsVariant::Integer32(y)) => x
                .checked_add(y)
                .map_or_else(|| Self::new(f64::from(x) + f64::from(y)), Self::new),
            (JsVariant::Float64(x), JsVariant::Float64(y)) => Self::new(x + y),
            (JsVariant::Integer32(x), JsVariant::Float64(y)) => Self::new(f64::from(x) + y),
            (JsVariant::Float64(x), JsVariant::Integer32(y)) => Self::new(x + f64::from(y)),
            (JsVariant::BigInt(x), JsVariant::BigInt(y)) => Self::new(JsBigInt::add(&x, &y)),

            // String concat
            (JsVariant::String(x), JsVariant::String(y)) => Self::from(js_string!(&x, &y)),
```

<a id="ref-q1-3"></a>
### [3] `core/engine/src/value/operations.rs:18-24`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L18-L24)

```rust
            (JsVariant::Integer32(x), JsVariant::Integer32(y)) => x
                .checked_add(y)
                .map_or_else(|| Self::new(f64::from(x) + f64::from(y)), Self::new),
            (JsVariant::Float64(x), JsVariant::Float64(y)) => Self::new(x + y),
            (JsVariant::Integer32(x), JsVariant::Float64(y)) => Self::new(f64::from(x) + y),
            (JsVariant::Float64(x), JsVariant::Integer32(y)) => Self::new(x + f64::from(y)),
            (JsVariant::BigInt(x), JsVariant::BigInt(y)) => Self::new(JsBigInt::add(&x, &y)),
```

<a id="ref-q1-4"></a>
### [4] `core/engine/src/value/operations.rs:27`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L27)

```rust
            (JsVariant::String(x), JsVariant::String(y)) => Self::from(js_string!(&x, &y)),
```

<a id="ref-q1-5"></a>
### [5] `core/engine/src/value/operations.rs:30-32`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L30-L32)

```rust
            (_, _) => {
                let x = self.to_primitive(context, PreferredType::Default)?;
                let y = other.to_primitive(context, PreferredType::Default)?;
```

<a id="ref-q1-6"></a>
### [6] `core/engine/src/bytecompiler/expression/assign.rs:15-16`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/expression/assign.rs#L15-L16)

```rust
    pub(crate) fn compile_assign(&mut self, assign: &Assign, dst: &Register) {
        let mut compiler = self.position_guard(assign);
```

<a id="ref-q1-7"></a>
### [7] `core/engine/src/bytecompiler/expression/assign.rs:18-24`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/expression/assign.rs#L18-L24)

```rust
        if assign.op() == AssignOp::Assign {
            match Access::from_assign_target(assign.lhs()) {
                Ok(access) => {
                    compiler.access_set(access, |compiler| {
                        compiler.compile_expr(assign.rhs(), dst);
                        dst
                    });
```

<a id="ref-q1-8"></a>
### [8] `core/engine/src/value/operations.rs:31-32`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L31-L32)

```rust
                let x = self.to_primitive(context, PreferredType::Default)?;
                let y = other.to_primitive(context, PreferredType::Default)?;
```

<a id="ref-q1-9"></a>
### [9] `core/engine/src/object/jsobject.rs:557-561`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L557-L561)

```rust
    pub fn to_primitive(
        &self,
        context: &mut Context,
        preferred_type: PreferredType,
    ) -> JsResult<JsValue> {
```

<a id="ref-q1-10"></a>
### [10] `core/engine/src/object/jsobject.rs:562-563`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L562-L563)

```rust
        // a. Let exoticToPrim be ? GetMethod(input, @@toPrimitive).
        let Some(exotic_to_prim) = self.get_method(JsSymbol::to_primitive(), context)? else {
```

<a id="ref-q1-11"></a>
### [11] `core/engine/src/object/jsobject.rs:578-586`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L578-L586)

```rust
        let hint = match preferred_type {
            PreferredType::Default => js_string!("default"),
            PreferredType::String => js_string!("string"),
            PreferredType::Number => js_string!("number"),
        }
        .into();

        //    iv. Let result be ? Call(exoticToPrim, input, « hint »).
        let result = exotic_to_prim.call(&self.clone().into(), &[hint], context)?;
```

<a id="ref-q1-12"></a>
### [12] `core/engine/src/object/jsobject.rs:588-594`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L588-L594)

```rust
        //    v. If Type(result) is not Object, return result.
        //    vi. Throw a TypeError exception.
        if result.is_object() {
            Err(js_error!(
                TypeError: "method `[Symbol.toPrimitive]` cannot return an object"
            ))
        } else {
```

<a id="ref-q1-13"></a>
### [13] `core/engine/src/object/jsobject.rs:569`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L569)

```rust
            return self.ordinary_to_primitive(context, preferred_type);
```

<a id="ref-q1-14"></a>
### [14] `core/engine/src/object/jsobject.rs:641-649`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L641-L649)

```rust
        // 3. If hint is "string", then
        //    a. Let methodNames be « "toString", "valueOf" ».
        // 4. Else,
        //    a. Let methodNames be « "valueOf", "toString" ».
        let method_names = if hint == PreferredType::String {
            [js_string!("toString"), js_string!("valueOf")]
        } else {
            [js_string!("valueOf"), js_string!("toString")]
        };
```

<a id="ref-q1-15"></a>
### [15] `core/engine/src/object/jsobject.rs:645-649`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L645-L649)

```rust
        let method_names = if hint == PreferredType::String {
            [js_string!("toString"), js_string!("valueOf")]
        } else {
            [js_string!("valueOf"), js_string!("toString")]
        };
```

<a id="ref-q1-16"></a>
### [16] `core/engine/src/object/jsobject.rs:657-663`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L657-L663)

```rust
            if let Some(method) = method.as_callable() {
                // i. Let result be ? Call(method, O).
                let result = method.call(&self.clone().into(), &[], context)?;

                // ii. If Type(result) is not Object, return result.
                if !result.is_object() {
                    return Ok(result);
```

<a id="ref-q1-17"></a>
### [17] `core/engine/src/object/jsobject.rs:668-671`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L668-L671)

```rust
        // 6. Throw a TypeError exception.
        Err(JsNativeError::typ()
            .with_message("cannot convert object to primitive value")
            .into())
```

<a id="ref-q1-18"></a>
### [18] `core/engine/src/object/jsobject.rs:631-639`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L631-L639)

```rust
        let recursion_limiter = RecursionLimiter::new(self.as_ref());
        if recursion_limiter.live {
            // we're in a recursive object, bail
            return Ok(match hint {
                PreferredType::Number => JsValue::new(0),
                PreferredType::String => JsValue::new(js_string!()),
                PreferredType::Default => unreachable!("checked type hint in step 2"),
            });
        }
```

<a id="ref-q1-19"></a>
### [19] `core/engine/src/value/operations.rs:33-35`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L33-L35)

```rust
                match (x.variant(), y.variant()) {
                    (JsVariant::String(x), _) => Self::from(js_string!(&x, &y.to_string(context)?)),
                    (_, JsVariant::String(y)) => Self::from(js_string!(&x.to_string(context)?, &y)),
```

<a id="ref-q1-20"></a>
### [20] `core/engine/src/value/operations.rs:36-48`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L36-L48)

```rust
                    (_, _) => {
                        match (x.to_numeric(context)?, y.to_numeric(context)?) {
                            (Numeric::Number(x), Numeric::Number(y)) => Self::new(x + y),
                            (Numeric::BigInt(ref x), Numeric::BigInt(ref y)) => {
                                Self::new(JsBigInt::add(x, y))
                            }
                            (_, _) => return Err(JsNativeError::typ()
                                .with_message(
                                    "cannot mix BigInt and other types, use explicit conversions",
                                )
                                .into()),
                        }
                    }
```

<a id="ref-q1-21"></a>
### [21] `core/engine/src/value/operations.rs:34-35`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L34-L35)

```rust
                    (JsVariant::String(x), _) => Self::from(js_string!(&x, &y.to_string(context)?)),
                    (_, JsVariant::String(y)) => Self::from(js_string!(&x.to_string(context)?, &y)),
```

<a id="ref-q1-22"></a>
### [22] `core/engine/src/object/jsobject.rs:645-646`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/object/jsobject.rs#L645-L646)

```rust
        let method_names = if hint == PreferredType::String {
            [js_string!("toString"), js_string!("valueOf")]
```

<a id="ref-q1-23"></a>
### [23] `core/engine/src/builtins/function/mod.rs:841`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L841)

```rust
    fn to_string(this: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
```

<a id="ref-q1-24"></a>
### [24] `core/engine/src/builtins/function/mod.rs:861-874`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L861-L874)

```rust
        if object.is::<NativeFunctionObject>() {
            let name = {
                // Is there a case here where if there is no name field on a value
                // name should default to None? Do all functions have names set?
                let value = object.get(js_string!("name"), &mut *context)?;
                if value.is_null_or_undefined() {
                    js_string!()
                } else {
                    value.to_string(context)?
                }
            };
            return Ok(
                js_string!(js_str!("function "), &name, js_str!("() { [native code] }")).into(),
            );
```

<a id="ref-q1-25"></a>
### [25] `core/engine/src/builtins/function/mod.rs:883-886`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/builtins/function/mod.rs#L883-L886)

```rust
        let code = function.codeblock();
        if let Some(code_points) = code.source_info().text_spanned().to_code_points() {
            return Ok(JsString::from(code_points).into());
        }
```

<a id="ref-q1-26"></a>
### [26] `core/engine/src/value/operations.rs:521-523`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L521-L523)

```rust
    /// are performed upon `x` and `y`. It is necessary because ECMAScript specifies left to right evaluation
    /// of expressions. The default value of `LeftFirst` is `true` and indicates that the `x` parameter
    /// corresponds to an expression that occurs to the left of the `y` parameter's corresponding expression.
```

<a id="ref-q1-27"></a>
### [27] `core/engine/src/value/operations.rs:534-556`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/value/operations.rs#L534-L556)

```rust
        left_first: bool,
        context: &mut Context,
    ) -> JsResult<AbstractRelation> {
        Ok(match (self.variant(), other.variant()) {
            // Fast path (for some common operations):
            (JsVariant::Integer32(x), JsVariant::Integer32(y)) => (x < y).into(),
            (JsVariant::Integer32(x), JsVariant::Float64(y)) => Number::less_than(f64::from(x), y),
            (JsVariant::Float64(x), JsVariant::Integer32(y)) => Number::less_than(x, f64::from(y)),
            (JsVariant::Float64(x), JsVariant::Float64(y)) => Number::less_than(x, y),
            (JsVariant::BigInt(x), JsVariant::BigInt(y)) => (x < y).into(),

            // Slow path:
            (_, _) => {
                let (px, py) = if left_first {
                    let px = self.to_primitive(context, PreferredType::Number)?;
                    let py = other.to_primitive(context, PreferredType::Number)?;
                    (px, py)
                } else {
                    // NOTE: The order of evaluation needs to be reversed to preserve left to right evaluation.
                    let py = other.to_primitive(context, PreferredType::Number)?;
                    let px = self.to_primitive(context, PreferredType::Number)?;
                    (px, py)
                };
```

<a id="ref-q1-28"></a>
### [28] `core/engine/src/bytecompiler/expression/binary.rs:56-73`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/core/engine/src/bytecompiler/expression/binary.rs#L56-L73)

```rust
    fn compile_binary_arithmetic(
        &mut self,
        op: ArithmeticOp,
        rhs_expr: &Expression,
        dst: &Register,
        lhs: RegisterOperand,
    ) {
        self.compile_expr_operand(rhs_expr, |self_, rhs| {
            let bytecode = &mut self_.bytecode;
            match op {
                ArithmeticOp::Add => bytecode.emit_add(dst.variable(), lhs, rhs),
                ArithmeticOp::Sub => bytecode.emit_sub(dst.variable(), lhs, rhs),
                ArithmeticOp::Div => bytecode.emit_div(dst.variable(), lhs, rhs),
                ArithmeticOp::Mul => bytecode.emit_mul(dst.variable(), lhs, rhs),
                ArithmeticOp::Exp => bytecode.emit_pow(dst.variable(), lhs, rhs),
                ArithmeticOp::Mod => bytecode.emit_mod(dst.variable(), lhs, rhs),
            }
        });
```

<a id="ref-q1-29"></a>
### [29] `Architecture:49-52`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/Architecture#L49-L52)

<a id="ref-q1-30"></a>
### [30] `JavaScript Values:1-10`
Source: [boa-dev/boa @ f5e88de5](https://github.com/boa-dev/boa/blob/f5e88de5/JavaScript Values#L1-L10)

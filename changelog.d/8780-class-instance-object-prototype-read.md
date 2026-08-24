Fixed inherited `Object.prototype` property reads on declared class instances.
Classes without their own `toString` or `valueOf` now resolve the default
methods instead of reporting the properties as present while reading them as
`undefined`, restoring ordinary string coercion such as `String(new C())`.

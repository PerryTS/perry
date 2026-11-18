On the web target a function expression or object-literal method called as a
method, or through `Reflect.apply`, reads its receiver as `this`:
`({ k: 5, g() { return this.k; } }).g()` returns 5.

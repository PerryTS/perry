On the web target a function expression or object-literal method called as a
method reads its receiver as `this`: `({ k: 5, g() { return this.k; } }).g()`
returns 5, and a method copied to another object reads that object.

const sum = new Function("a", "b", "return a+b");
console.log(sum(17, 25));
console.log(Function("return this")() === globalThis);
console.log(Function('"use strict"; ' + 'return 6 * 7')());
function scope() {
    const callerOnly = 123;
    console.log(Function("return typeof callerOnly")());
    console.log(callerOnly);
}
scope();

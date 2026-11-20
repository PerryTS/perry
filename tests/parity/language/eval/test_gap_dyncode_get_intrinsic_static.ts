// get-intrinsic's idiom: a private helper builds source from a finite set of
// literal pieces. Every case is constant source compiled statically.
var $Function = Function;
var getEvalledConstructor = function (expressionSyntax: string) {
    try {
        return $Function('"use strict"; return (' + expressionSyntax + ').constructor;')();
    } catch (e) {}
};
var table: Record<string, any> = { "%Function%": $Function };
var doEval = function doEval(name: string) {
    var value;
    if (name === "%AsyncFunction%") {
        value = getEvalledConstructor("async function () {}");
    } else if (name === "%GeneratorFunction%") {
        value = getEvalledConstructor("function* () {}");
    } else if (name === "%AsyncGeneratorFunction%") {
        value = getEvalledConstructor("async function* () {}");
    }
    table[name] = value;
    return value;
};
for (const n of ["%AsyncFunction%", "%GeneratorFunction%", "%AsyncGeneratorFunction%"]) {
    const ctor = doEval(n);
    console.log(n, typeof ctor);
}
console.log(table["%Function%"] === Function, Function("return this")() === globalThis);
console.log([3, 1, 2].sort().join(","));

// Sloppy CommonJS: a direct eval of runtime source declares its `var`s in
// the calling function's scope; native code after the eval sees them, and an
// enclosing binding of the same name is shadowed only inside that function.
var shadowed = "module";
function make(source: string) {
    eval(source);
    var seen = typeof created === "undefined" ? "missing" : created;
    created = seen + 1;
    return [seen, eval("created"), shadowed];
}
function nested(source: string) {
    eval(source);
    return function () {
        return created;
    };
}
function untouched(source: string) {
    eval(source);
    return typeof created;
}
const source = process.argv[2] || "var created = 7; var shadowed = 'eval';";
console.log(JSON.stringify(make(source)));
console.log(typeof (globalThis as any).created, shadowed);
console.log(nested(process.argv[2] || "var created = 7")());
console.log(untouched(process.argv[3] || "1 + 1"));

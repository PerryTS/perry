// G8, sloppy half (CommonJS: every function is non-strict, so simple
// parameters the body names are mapped into the Arguments object). Forwarding
// `...arguments` directly is only exact while every mapped parameter still
// holds its incoming value; otherwise the forwarded list must show the write.
// Compared against Node.

const show = (...xs: any[]) => JSON.stringify(xs.map((x) => (x === undefined ? "u" : x)));
const sink: any = { m(...xs: any[]) { return show(...xs); } };

// Untouched mapped parameters: the elided shape.
function untouched(a: any, b: any) { return sink.m(...arguments) + a + b; }
console.log((untouched as any)(1, 2, 3), (untouched as any)(1));

// A mapped parameter written before the spread: the write is forwarded.
function written(a: any, b: any) { a = "w"; return sink.m(...arguments) + b; }
console.log((written as any)(1, 2, 3), (written as any)());
function writtenAfter(a: any) { const r = sink.m(...arguments); a = "late"; return r + sink.m(...arguments); }
console.log((writtenAfter as any)(1, 2));
function compound(a: any) { a += 10; a++; return sink.m(...arguments); }
console.log((compound as any)(1, 2));
// Written by a closure.
function viaClosure(a: any) { const set = (v: any) => { a = v; }; set(9); return sink.m(...arguments); }
console.log((viaClosure as any)(1, 2));
// Written through the object.
function throughObject(a: any) { arguments[0] = "o"; return sink.m(...arguments) + a; }
console.log((throughObject as any)(1, 2));
// A parameter only an unpassed index would map.
function unpassed(a: any, b: any) { b = "b!"; return sink.m(...arguments) + arguments.length; }
console.log((unpassed as any)(1));
// Duplicate parameter names (the last one is the binding).
function dup(a: any, a2: any) { return sink.m(...arguments); }
console.log((dup as any)(1, 2, 3));
// Non-simple parameters: unmapped even in sloppy code.
function nonSimple(a: any = 0, b?: any) { a = "x"; return sink.m(...arguments) + a; }
console.log((nonSimple as any)(1, 2));
function destructured({ p }: any, b: any) { b = "y"; return sink.m(...arguments) + p; }
console.log((destructured as any)({ p: 5 }, 2));

// Edits to the object itself are observed by the spread.
function lengthSet() { arguments.length = 1; return sink.m(...arguments); }
console.log((lengthSet as any)(1, 2, 3));
function deleted() { delete arguments[0]; return sink.m(...arguments); }
console.log((deleted as any)(1, 2));
function extraKey() { (arguments as any).extra = 1; return sink.m(...arguments) + Object.keys(arguments).join(""); }
console.log((extraKey as any)(1, 2));
function escapes(this: any) { const keep = arguments; return sink.m(...arguments) + keep.length; }
console.log((escapes as any)(1, 2));
function callee() { return sink.m(...arguments) + (arguments.callee === callee); }
console.log((callee as any)(1));

// `arguments` shadowed: a parameter, a var, a function declaration, a block let.
function asParam(arguments: any) { return sink.m(...arguments); }
console.log(asParam([7, 8]));
function asVar() { var arguments: any = ["v"]; return sink.m(...arguments); }
console.log((asVar as any)(1, 2));
function asLet() { { let arguments: any = ["l"]; return sink.m(...arguments); } }
console.log((asLet as any)(1, 2));
function asFn() { function arguments() {} return typeof arguments; }
console.log((asFn as any)(1));
function assigned() { const first = sink.m(...arguments); (arguments as any) = [3]; return first + sink.m(...arguments); }
console.log((assigned as any)(1, 2));

// The three.js WebGLState shape (sloppy function in an object literal).
const gl: any = { tex(a: any, b: any) { return show(a, b, arguments.length); } };
const state: any = { tex: function () { try { gl.tex(...arguments); return gl.tex(...arguments); } catch (e) { return "caught"; } } };
console.log(state.tex(1, 2, 3), state.tex());

// #11724: receiver and lexical captures must survive every loop shape.
const object = {
    d: 7,
    doLoop(n: number) { let r = 0; do { r += this.d; n--; } while (n > 0); return r; },
    whileLoop(n: number) { let r = 0; while (n-- > 0) { r += this.d; } return r; },
    forLoop(n: number) { let r = 0; for (let i = 0; i < n; i++) { r += this.d; } return r; },
    forIn() { let r = 0; for (const k in { a: 1, b: 2 }) { r += this.d; } return r; },
    forOf() { let r = 0; for (const v of [1, 2]) { r += this.d; } return r; },
    labeled(n: number) { let r = 0; outer: do { r += this.d; n--; continue outer; } while (n > 0); return r; },
    continued(n: number) { let r = 0; do { n--; if (n === 1) continue; r += this.d; } while (n > 0); return r; },
    condition() { let n = 0; do { n++; } while (n < this.d); return n; },
    arrow() { const f = () => { let r = 0; do { r += this.d; } while (false); return r; }; return f(); },
    nestedArrow() { const f = () => { let r = 0; outer: do { const g = () => this.d; r += g(); } while (false); return r; }; return f(); },
    plain: function () { let r = 0; do { r += this.d; } while (false); return r; },
};
console.log(object.doLoop(1), object.doLoop(3), object.whileLoop(2), object.forLoop(2));
console.log(object.forIn(), object.forOf(), object.labeled(2), object.continued(3));
console.log(object.condition(), object.arrow(), object.nestedArrow(), object.plain());
function declared() { let r = 0; do { r += this.d; } while (false); return r; }
console.log(declared.call(object));
class C { d = 7; f() { let r = 0; do { r += this.d; } while (false); return r; } }
console.log(new C().f());
function args(a: number, b: number) {
    let r = 0;
    do { r += arguments[0] + arguments[1]; } while (false);
    const f = () => { let v = 0; do { v = arguments[1]; } while (false); return v; };
    return r + f();
}
console.log(args(3, 4));
const argsExpr = function (a: number) { let r = 0; do { r = arguments[0]; } while (false); return r; };
console.log(argsExpr(9));
function Target() {
    let direct = false;
    do { direct = new.target === Target; } while (false);
    const f = () => { let captured = false; outer: do { captured = new.target === Target; } while (false); return captured; };
    console.log(direct, f());
}
Target();
new Target();

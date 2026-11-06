const a = Symbol("a"), b = Symbol("b"), c = Symbol("c");
const x = Symbol("x"), y = Symbol("y"), z = Symbol("z");
function mk(key: symbol, access: symbol) {
    return class {
        value = 1;
        [key]() { return this.value; }
        static [key]() { return 2; }
        get [access]() { return this.value; }
        set [access](value: number) { this.value = value; }
        static value = 3;
        static get [access]() { return this.value; }
        static set [access](value: number) { this.value = value; }
    };
}
const A = mk(a, x), B = mk(b, y), C = mk(c, z);
function keys(value: any) {
    return Object.getOwnPropertySymbols(value).map(String).join();
}
console.log("instance-keys", keys(A.prototype), keys(B.prototype), keys(C.prototype));
console.log("static-keys", keys(A), keys(B), keys(C));
const ai = new A(), bi = new B(), ci = new C();
console.log("methods", ai[a](), bi[b](), ci[c](), A[a](), B[b](), C[c]());
ai[x] = 4; bi[y] = 5; ci[z] = 6;
A[x] = 7; B[y] = 8; C[z] = 9;
console.log("accessors", ai[x], bi[y], ci[z], A[x], B[y], C[z]);
console.log("setter-receivers", ai[a](), bi[b](), ci[c](), A.value, B.value, C.value);
console.log("absent", typeof ci[a], typeof C[a], typeof ci[x], typeof C[x]);
class Sub extends C {
    inherited() { return super[c](); }
}
console.log("super", new Sub().inherited());
console.log("delete", delete C.prototype[c], keys(C.prototype), typeof ci[c]);
try { ci[c](); console.log("deleted-call-succeeded"); }
catch (error) { console.log("deleted-call", error instanceof TypeError); }
const once = Symbol("once"), field = Symbol("field");
class Once {
    value = 10;
    [once]() { return this.value; }
    static [once]() { return 11; }
    get [field]() { return this.value; }
    set [field](value: number) { this.value = value; }
    static value = 12;
    static get [field]() { return this.value; }
    static set [field](value: number) { this.value = value; }
}
const oi = new Once(); oi[field] = 13; Once[field] = 14;
console.log("declaration", keys(Once.prototype), keys(Once), oi[once](), Once[once](), oi[field], Once[field]);

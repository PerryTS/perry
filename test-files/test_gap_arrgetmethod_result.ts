const a: any = [1,2,3,4];
const original = Array.prototype[Symbol.iterator];
const p: any = Object.getPrototypeOf(a.values());
const originalNext = p.next;
for (const kind of ["own", "prototype", "next"]) {
    for(const result of [null, undefined, 7, "bad"]) {
        if (kind === "own") a[Symbol.iterator] = function() {return {next() {return result;}};};
        if (kind === "prototype") Array.prototype[Symbol.iterator] = function() {return {next() {return result;}};};
        if (kind === "next") p.next = function() {return result;};
        try { for(const x of a) console.log("unexpected", x); } catch(e: any) {console.log(kind, "for", e instanceof TypeError);}
        try { const [x] = a; console.log("unexpected", x); } catch(e: any) {console.log(kind, "binding", e instanceof TypeError);}
        delete a[Symbol.iterator];
        Array.prototype[Symbol.iterator] = original;
        p.next = originalNext;
    }
}

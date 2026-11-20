const a: any = [1,2,3,4];
// DEC 250: Array.prototype and %ArrayIteratorPrototype% are locked; the "prototype" kind
// replaces the inherited iterator on a user prototype link (same lookup), "next" is refused.
const uap: any = Object.create(Array.prototype);
const p: any = Object.getPrototypeOf(a.values());
const originalNext = p.next;
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
for (const kind of ["own", "prototype", "next"]) {
    for(const result of [null, undefined, 7, "bad"]) {
        if (kind === "own") a[Symbol.iterator] = function() {return {next() {return result;}};};
        if (kind === "prototype") { uap[Symbol.iterator] = function() {return {next() {return result;}};}; Object.setPrototypeOf(a, uap); }
        if (kind === "next") locked(() => { p.next = function() {return result;}; });
        try { for(const x of a) console.log("unexpected", x); } catch(e: any) {console.log(kind, "for", e instanceof TypeError);}
        try { const [x] = a; console.log("unexpected", x); } catch(e: any) {console.log(kind, "binding", e instanceof TypeError);}
        delete a[Symbol.iterator];
        Object.setPrototypeOf(a, Array.prototype);
        delete uap[Symbol.iterator];
        if (kind === "next") locked(() => { p.next = originalNext; });
    }
}

// #6981: a hole read must still terminate after `Array.prototype` RELOCATED.
//
// Writing an index past `Array.prototype`'s dense capacity reallocates its
// backing store (`js_array_grow`) and leaves a GC forwarding stub at the old
// head — with no GC involved at all. The runtime memoizes `Array.prototype`'s
// address, and the hole/OOB read fallback guards against self-recursion with
// the object-identity test `proto != receiver`. Readers resolve their receiver
// through the forwarding chain, so a memoized address that does not resolve
// names the same object by a different address, the guard stops firing, and the
// element read recurses until the stack guard page (SIGSEGV).
//
// Three conditions, all present below and each individually necessary: an
// `Array.prototype` INDEX WRITE (naming it is not enough), a HOLE READ (an
// index never assigned), and a prototype that has MOVED.
// DEC 250: Array.prototype refuses program index adds, so it can no longer
// grow or move; the refused add is the expectation, the hole reads still run.
const proto: any = Array.prototype;
try { proto[300] = 555; } catch (e) { console.log("DEC 250: " + (e as Error).message); }

const c: number[] = new Array(4);
c[0] = 1;
console.log("" + c[1]);
console.log("" + c[300]);
console.log((c[1] as any) || -1);

// A hole read on an index the prototype *does* carry inherits its value.
try { proto[2] = 777; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log("" + c[2]);
console.log(c[0] + ((c[2] as any) || 0));

console.log("" + c[2], "" + c[300]);

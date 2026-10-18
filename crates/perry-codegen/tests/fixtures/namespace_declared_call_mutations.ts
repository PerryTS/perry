namespace Calls {
    export let level = 1;
    export let enabled = false;
    export function f(v = 0) { return "original:" + v; }
}
function replacement(v = 0) { return "replacement:" + v; }
const original = Calls.f;
console.log(Calls.f());
Calls.level = 2;
Calls.enabled = true;
console.log(Calls.f());
(Calls as any)["f"] = replacement;
console.log(Calls.f());
(Calls as any)["f"] = original;
console.log(Calls.f());
console.log(Calls.f(((Calls as any)["f"] = replacement, 7)));
(Calls as any)["f"] = original;
Object.defineProperty(Calls, "f", {
    configurable: true,
    get() { console.log("get"); return replacement; }
});
console.log(Calls.f((console.log("argument"), 8)));
delete (Calls as any)["f"];
console.log(typeof (Calls as any)["f"]);
try { Calls.f(); } catch (error) { console.log((error as Error).name); }
(Calls as any)["f"] = original;
console.log(Calls.f(9));

namespace PointerExports {
    export let first = { value: 13 };
    export let second = [14];
    export function read() { return first.value + second[0]; }
}
console.log(PointerExports.read(), JSON.stringify(PointerExports.first), JSON.stringify(PointerExports.second));
PointerExports.second = [15];
console.log(PointerExports.read(), JSON.stringify(PointerExports.second));

function make(v: number) {
    const object: any = {};
    object.level = 1;
    object.enabled = false;
    object.f = () => v;
    return object;
}
const first = make(11);
const second = make(12);
console.log(first.f(), second.f());
first.f = second.f;
console.log(first.f());

const literal: any = {};
literal.level = 1;
literal.enabled = false;
literal.f = function () { return this.level; };
console.log(literal.f());
literal.level = 42;
console.log(literal.f());

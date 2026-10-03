// Object() allocates a classless runtime record. Appending value learns F64,
// while the record still lacks the plain-data store birth mark.
function count(check: any): number {
    let hits = 0;
    for (let i = 0; i < 12; i++) {
        try {
            if (i < check.value) hits++;
        } catch {
            hits += 100;
        }
    }
    return hits;
}

const check: any = Object();
check.kind = "min";
check.value = 5;
console.log("numeric", count(check), count(check));
check.value = "7";
console.log("generalized", count(check));
let getterCalls = 0;
Object.defineProperty(check, "value", {
    configurable: true,
    get() { getterCalls++; return 3; },
});
console.log("accessor", count(check), getterCalls);

const altered: any = Object();
altered.value = 4;
console.log("before-prototype", count(altered));
Object.setPrototypeOf(altered, null);
console.log("null-prototype", count(altered));
delete altered.value;
console.log("absent", count(altered));

const left: any = Object();
left.value = 8;
const right: any = Object();
right.value = 6;
let changed = 0;
Object.defineProperty(left, "value", {
    get() { changed++; right.value = changed % 2 ? 4 : "9"; return 5; },
});
let total = 0;
for (let i = 0; i < 10; i++) {
    try {
        if (left.value < right.value) total++;
    } catch {
        total += 100;
    }
}
console.log("left-before-guard", total, changed);

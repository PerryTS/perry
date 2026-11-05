// Number facts must survive scalar replacement and remain valid across every
// field write. Erased annotations and a numeric initializer are insufficient.
function numeric(n: number) {
    const object = { x: 1, y: 0 };
    let sum = 0;
    for (let i = 0; i < n; i++) {
        object.y = i;
        sum += object.x;
    }
    return sum + object.y;
}

function mixed(branch: boolean, value: any) {
    const object: { x: number; y: number } = { x: 1, y: 0 };
    if (branch) object.x = value;
    let sum: any = 0;
    for (let i = 0; i < 4; i++) {
        object.y = i;
        sum += object.x;
    }
    return sum;
}

let conversions = 0;
const convertible = {
    valueOf() {
        conversions++;
        return 3;
    }
};
console.log(numeric(64));
console.log(mixed(false, "s"));
console.log(mixed(true, "s"));
console.log(mixed(true, undefined));
console.log(mixed(true, null));
console.log(mixed(true, convertible), conversions);

const receiver: any = { first: 7, value: undefined };
const name = ["val", "ue"].join("");
console.log(receiver.first, receiver[name], Object.hasOwn(receiver, name));
Object.defineProperty(receiver, "value", { get() { return 13; }, configurable: true });
console.log(receiver[name]);
delete receiver.value;
Object.setPrototypeOf(receiver, { value: 17 });
console.log(receiver[name]);

class GuardedCounter10769 {
    x = 1;
    y = 0;
    run(n: any) {
        let h = 0;
        for (let i = 0; i < n; i++) {
            this.y = i;
            h += this.x;
        }
        return [h, this.y];
    }
}
for (const bound of [3, "3", undefined, null, NaN, 3n]) {
    console.log("bound", String(bound), JSON.stringify(new GuardedCounter10769().run(bound)));
}
let boundCalls10769 = 0;
const coercingBound10769 = { valueOf() { boundCalls10769++; return 3; } };
console.log("bound-object", JSON.stringify(new GuardedCounter10769().run(coercingBound10769)), boundCalls10769);
let getterCalls10769 = 0;
const accessorCounter10769 = new GuardedCounter10769();
Object.defineProperty(accessorCounter10769, "x", { get() { getterCalls10769++; return 2; } });
console.log("bound-accessor", JSON.stringify(accessorCounter10769.run(coercingBound10769)), getterCalls10769, boundCalls10769);

function receiverBound10769(o: any) {
    let h: any = 0;
    for (let i = 0; i < o; i++) {
        o.y = i;
        h += o.x;
    }
    return [h, o.y];
}
let receiverBoundCalls10769 = 0;
const receiverAndBound10769 = {
    x: 1 as any,
    y: 0,
    valueOf() {
        receiverBoundCalls10769++;
        if (receiverBoundCalls10769 === 2) this.x = "s";
        return 3;
    }
};
console.log("receiver-bound", JSON.stringify(receiverBound10769(receiverAndBound10769)), receiverBoundCalls10769);

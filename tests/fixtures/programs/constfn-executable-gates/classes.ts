function later(): number {
    let total = 0;
    for (let i = 0; i < 128; i++) { const p = { x: i }; total += p.x; }
    return total;
}
class User {
    x = 1;
    m = () => this.x;
    effect = later();
    constructor(x: number) { this.x = x; later(); }
    tick() { this.x += 1; return this.x; }
    resetMethod() { this.m = () => 101; }
}
function callM(receiver: any) { return receiver.m(); }
const first = new User(11), second = new User(23);
let sum = 0;
for (let i = 0; i < 1024; i++) {
    if (i % 2) { second.tick(); sum += callM(second); }
    else { first.tick(); sum += callM(first); }
}
console.log(sum, callM(first), callM(second), first.m !== second.m);
first.resetMethod();
console.log(callM(first), callM(second));
console.log(first.effect, second.effect);

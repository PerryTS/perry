class Box { x = 1; y = 2; m() { console.log("method"); return this.x + this.y; } }
function call(b: Box) { return b.m(); }
const b = new Box();
delete (b as any).x;
console.log(call(b));

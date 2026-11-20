// Closes #304: `arr.length = N` must truncate (and clear elements past N) /
// extend per JS spec. Pre-fix Perry routed the assignment through the generic
// PropertySet path which only recorded a "length" key on the array's hidden
// object dispatch — the real ArrayHeader.length field stayed unchanged and
// the indexed elements were never cleared. CommandBuffer-style swap-buffer
// patterns silently re-processed the same commands on every flush.

console.log("=== truncate to 0 ===");
const a = [1, 2, 3];
a.length = 0;
console.log(`length: ${a.length}`);
console.log(`a[0]: ${a[0]}, a[1]: ${a[1]}, a[2]: ${a[2]}`);

console.log("=== truncate to 2 ===");
const b = [10, 20, 30, 40, 50];
b.length = 2;
console.log(`length: ${b.length}`);
console.log(`b[0]: ${b[0]}, b[1]: ${b[1]}, b[2]: ${b[2]}, b[3]: ${b[3]}`);

console.log("=== no-op (length === current) ===");
const c = [1, 2, 3];
c.length = 3;
console.log(`length: ${c.length}, c[0]: ${c[0]}, c[2]: ${c[2]}`);

console.log("=== extend (in-capacity) ===");
const d = [1, 2, 3];
d.length = 5;
console.log(`length: ${d.length}`);
console.log(`d[0]: ${d[0]}, d[1]: ${d[1]}, d[2]: ${d[2]}, d[3]: ${d[3]}, d[4]: ${d[4]}`);

console.log("=== push after truncate works ===");
const e = [1, 2, 3, 4, 5];
e.length = 0;
e.push(99);
console.log(`length: ${e.length}, e[0]: ${e[0]}`);

console.log("=== string array truncate ===");
const s = ["a", "b", "c"];
s.length = 1;
console.log(`length: ${s.length}, s[0]: ${s[0]}, s[1]: ${s[1]}`);

console.log("=== CommandBuffer swap-buffer pattern (issue's actual repro) ===");
class CommandBuffer {
  private commands: number[] = [];
  private swapBuffer: number[] = [];

  push(cmd: number): void {
    this.commands.push(cmd);
  }

  flush(): number[] {
    const current = this.commands;
    this.commands = this.swapBuffer;
    const out = current.slice();
    current.length = 0;
    this.swapBuffer = current;
    return out;
  }
}
const buf = new CommandBuffer();
buf.push(1);
buf.push(2);
const f1 = buf.flush();
console.log(`flush 1: [${f1.join(",")}]`);
buf.push(3);
const f2 = buf.flush();
console.log(`flush 2: [${f2.join(",")}]`);
const f3 = buf.flush();
console.log(`flush 3 (empty): [${f3.join(",")}]`);

// The text detector selects the typed-array group for this program by a
// name (`CommandBuffer`); the whole-program fact says no byte cell is ever
// created or observed. The link decision leaves the group out.
class CommandBuffer {
  private commands: number[] = [];
  private swapBuffer: number[] = [];
  push(n: number): void {
    this.commands.push(n);
  }
  flush(): number {
    const current = this.commands;
    this.commands = this.swapBuffer;
    this.commands.length = 0;
    this.swapBuffer = current;
    let sum = 0;
    for (const n of current) sum += n;
    return sum;
  }
}
const buffer = new CommandBuffer();
for (let i = 0; i < 100; i++) buffer.push((i * 37) % 101);
console.log(buffer.flush(), buffer.flush());
// More than 64 elements with a comparator: the sort's private index cells.
const values: number[] = [];
for (let i = 0; i < 200; i++) values.push((i * 7919) % 211);
values.sort((a, b) => a - b);
console.log(values[0], values[99], values[199]);

// Regression for #13649: bounded HIR unrolling of a nested formatter.
// Standalone synthetic table formatter; no application code or dependencies.
class Table {
  values: number[];
  selected: number;
  color: boolean;

  constructor(seed: number, color: boolean) {
    this.values = [];
    this.selected = seed & 63;
    this.color = color;
    for (let i = 0; i < 64; i++) this.values.push((seed + i * 17) % 100);
  }

  render(): string {
    const lines: string[] = [];
    for (let row = 0; row < 8; row++) {
      let line = String(row) + ":";
      for (let col = 0; col < 8; col++) {
        const index = row * 8 + col;
        const value = this.values[index] ?? 0;
        let label = String(value);
        if (value < 10) label = "0" + label;
        const selected = index === this.selected;
        if (this.color) {
          let background = ((row + col) & 1) === 0 ? 24 : 30;
          if (value > 70) background = 52;
          if (selected) background = 100;
          line += "\x1b[48;5;" + String(background) + "m " + label + " \x1b[0m";
        } else line += selected ? "[" + label + "]" : " " + label + " ";
      }
      lines.push(line);
    }
    return lines.join("\n");
  }
}

const table = new Table(Number(process.argv[2] ?? "7"), process.argv[3] === "color");
console.log(table.render());

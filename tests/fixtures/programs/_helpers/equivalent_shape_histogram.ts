export class Histogram {
  bins: number[] = new Array<number>(8).fill(0);
  count: number = 0;
  add(index: number): void {
    this.bins[index & 7] = (this.bins[index & 7] ?? 0) + 1;
    this.count++;
  }
  score(): number {
    let result = this.count;
    for (let i = 0; i < 8; i++) {
      const value = this.bins[i] ?? 0;
      result += value * (i + 1);
    }
    return result;
  }
  text(): string {
    let result = "";
    for (let i = 0; i < 8; i++) result += String(this.bins[i] ?? 0) + ":";
    return result;
  }
}

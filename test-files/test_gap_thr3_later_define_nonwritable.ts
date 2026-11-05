// THR3: a key stored first, then made non-writable by defineProperty; a
// method's strict store must throw (the layout word must miss).
class L {
  k: number;
  constructor(lock: boolean) {
    this.k = 1;
    if (lock) Object.defineProperty(this, "k", { writable: false });
  }
  put(v: number): string {
    try {
      this.k = v;
      return "ok:" + this.k;
    } catch (e) {
      return (e as Error).constructor.name + ":" + this.k;
    }
  }
}
const out: string[] = [];
for (let i = 0; i < 4; i++) out.push(new L(i % 2 === 1).put(i + 10));
const m = new L(false);
out.push(m.put(3));
Object.defineProperty(m, "k", { writable: false });
out.push(m.put(4));
console.log(out.join(" "));

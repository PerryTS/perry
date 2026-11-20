// Two "module factories" (as a bundler emits them into one file) each declare a
// class named `L`. Perry flattens them and scope-renames the second `L`. An
// `x instanceof L` inside the second factory must still resolve to THAT
// factory's `L` (its own class), not the first factory's same-named class.
// Regression: the rename was applied to `class`/`extends`/`new` but NOT to the
// `instanceof` operand, so `instanceof L` matched the wrong class_id and
// returned false even though the prototype chain was correct.
const factories: Array<(r: any) => void> = [
  (r) => {
    class L {
      seal(): string { return "util"; }
    }
    r.Util = L;
  },
  (r) => {
    class L {}
    class D extends L {}
    class G extends D {}
    const g = new G();
    r.iofL = g instanceof L;
    r.iofD = g instanceof D;
  },
];
const out: any = {};
for (const f of factories) f(out);
console.log("iofL=" + out.iofL);
console.log("iofD=" + out.iofD);

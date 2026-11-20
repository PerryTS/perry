// #8691: fixed arrays of plain descriptor objects passed to a known helper
// must retain JS semantics when the inliner, static-loop unroller, and escape
// proof remove both carrier identities.

class Position {}
class Velocity {}

let checksum = 0;

function consume(initializers: { component: unknown }[]): void {
  for (let i = 0; i < initializers.length; i++) {
    const initializer = initializers[i];
    if (initializer.component === Position) checksum += 1;
    if (initializer.component === Velocity) checksum += 2;
  }
}

const iterations = 500_000;
for (let i = 0; i < iterations; i++) {
  consume([{ component: Position }, { component: Velocity }]);
  consume([{ component: Position }]);
  consume([{ component: Velocity }]);
}

console.log(checksum);

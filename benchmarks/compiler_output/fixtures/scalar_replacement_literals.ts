function scalarReplacementChecksum(): number {
  const point = { x: 1.25, y: 2.5, z: 4.0 };
  const base = point.x + point.y;
  const z0 = point.z;
  point.z = base + z0;

  const x0 = point.x;
  const z1 = point.z;
  const values = [x0, z1, 5.0];
  return values[0] + values[1] + values[2] + values.length;
}

class Position {}
class Velocity {}

let aggregateChecksum = 0;

function consumeAggregate(initializers: { component: unknown }[]): void {
  for (let i = 0; i < initializers.length; i++) {
    const initializer = initializers[i];
    if (initializer.component === Position) aggregateChecksum += 1;
    if (initializer.component === Velocity) aggregateChecksum += 2;
  }
}

function scalarAggregateCallChecksum(): number {
  aggregateChecksum = 0;
  const iterations = 500_000;
  for (let i = 0; i < iterations; i++) {
    consumeAggregate([{ component: Position }, { component: Velocity }]);
    consumeAggregate([{ component: Position }]);
    consumeAggregate([{ component: Velocity }]);
  }
  return aggregateChecksum;
}

console.log(scalarReplacementChecksum());
console.log(scalarAggregateCallChecksum());

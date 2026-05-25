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

console.log(scalarReplacementChecksum());

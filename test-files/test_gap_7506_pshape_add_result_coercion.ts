// A typed-receiver method call has three paths: dynamic method lookup, a
// raw-f64 receiver clone, and a Ptr<Shape> fallback when the method identity is
// stable but a numeric field no longer holds a raw double. The fallback may
// trust field offsets, but it must preserve JavaScript coercion semantics.

class Point7506 {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }

  score(scale: number): number {
    const sum = this.x + this.y;
    return sum * scale;
  }

  scoreInline(scale: number): number {
    return (this.x + this.y) * scale;
  }
}

function probe7506(receiver: Point7506, scale: number): number {
  return receiver.score(scale);
}

function probeInline7506(receiver: Point7506, scale: number): number {
  return receiver.scoreInline(scale);
}

function poison7506(receiver: Point7506): void {
  (receiver as any).x = "1";
}

// Recursion keeps the receiver as a real heap parameter. Without it the whole
// top-level scenario is inlined and scalar-replaced, so it never exercises the
// typed-receiver guard and its Ptr<Shape> fallback.
function run7506(receiver: Point7506, remaining: number): string {
  if (remaining > 0) return run7506(receiver, remaining - 1);
  poison7506(receiver);
  const result = probe7506(receiver, 3);
  const inline = probeInline7506(receiver, 3);
  return `${result} ${typeof result} ${inline} ${typeof inline}`;
}

console.log(run7506(new Point7506(1, 2), 1));

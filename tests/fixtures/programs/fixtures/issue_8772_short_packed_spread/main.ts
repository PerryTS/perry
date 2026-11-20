class Position {
  x: number;
  constructor() { this.x = 0; }
  reset(entity: { id: number }, delta: number = 1): void {
    this.x = entity.id + delta;
  }
}

class Velocity {
  dx: number;
  constructor() { this.dx = 0; }
  reset(entity: { id: number }, delta: number = 2): void {
    this.dx = entity.id + delta;
  }
}

const position = new Position();
const velocity = new Velocity();
const empty: number[] = [];
const one: number[] = [3];

function invoke(instance: any, entity: { id: number }, args: number[]): void {
  instance.reset(entity, ...args);
}

let checksum = 0;
for (let i = 0; i < 300000; i++) {
  const entity = { id: i };
  invoke(position, entity, empty);
  invoke(velocity, entity, one);
  checksum += position.x + velocity.dx;
}
console.log(checksum);

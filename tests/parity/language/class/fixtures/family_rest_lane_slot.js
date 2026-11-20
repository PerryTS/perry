// Fixture of test_gap_family_rest_lane_slot: plain JS classes (no field
// declarations), so `Ray` and `Caster` are static class families whose keys
// are their constructor stores.
export class Ray {
  constructor(origin) {
    this.origin = origin !== undefined ? origin : { x: 0 };
    this.direction = { x: 1 };
  }
}
// `camera` and `ray` sit at positions 0 and 1 here, so key colouring gives
// them those slots in every family that holds them.
export class Pair {
  constructor() {
    this.camera = 1;
    this.ray = 2;
  }
}
export class Triple {
  constructor() {
    this.camera = 3;
    this.ray = 4;
    this.far = 5;
  }
}
// Key order ray, near, far, camera: colouring moves `ray` off position 0 to
// slot 1, where `near` (an int32 field) is at position 1.
export class Caster {
  constructor(origin, near = 0, far = Infinity) {
    this.ray = new Ray(origin);
    this.near = near;
    this.far = far;
    this.camera = null;
  }
  aim(c) {
    this.ray.origin.x = c.v;
    this.camera = c;
  }
  // A writer in this module of a field the test module reads.
  setNear(n) {
    this.near = n | 0;
  }
  nearTimes(k) {
    return this.near * k;
  }
}

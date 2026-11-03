// @ts-check
// Copyright (c) 2001-2010, Purdue University. All rights reserved.
// Copyright (C) 2015 Apple Inc. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//  * Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
//  * Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//  * Neither the name of the Purdue University nor the
//    names of its contributors may be used to endorse or promote products
//    derived from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
// WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
// (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
// LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND
// ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

const { Benchmark } = require('./benchmark');
const som = require('./som');

class Vector2D {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }

  plus(other) {
    return new Vector2D(this.x + other.x, this.y + other.y);
  }

  minus(other) {
    return new Vector2D(this.x - other.x, this.y - other.y);
  }

  compareNumbers(a, b) {
    if (a === b) {
      return 0;
    }
    if (a < b) {
      return -1;
    }
    if (a > b) {
      return 1;
    }

    // We say that NaN is smaller than non-NaN.
    if (a === a) { // eslint-disable-line no-self-compare
      return 1;
    }
    return -1;
  }

  compareTo(other) {
    const result = this.compareNumbers(this.x, other.x);
    if (result) {
      return result;
    }
    return this.compareNumbers(this.y, other.y);
  }
}

class Vector3D {
  constructor(x, y, z) {
    this.x = x;
    this.y = y;
    this.z = z;
  }

  plus(other) {
    return new Vector3D(this.x + other.x, this.y + other.y, this.z + other.z);
  }

  minus(other) {
    return new Vector3D(this.x - other.x, this.y - other.y, this.z - other.z);
  }

  dot(other) {
    return this.x * other.x + this.y * other.y + this.z * other.z;
  }

  squaredMagnitude() {
    return this.dot(this);
  }

  magnitude() {
    return Math.sqrt(this.squaredMagnitude());
  }

  times(amount) {
    return new Vector3D(this.x * amount, this.y * amount, this.z * amount);
  }
}

class Entry {
  constructor(key, value) {
    this.key = key;
    this.value = value;
  }
}

class InsertResult {
  constructor(isNewEntry, newNode, oldValue) {
    this.isNewEntry = isNewEntry;
    this.newNode = newNode;
    this.oldValue = oldValue;
  }
}

function treeMinimum(x) {
  let current = x;
  while (current.left) {
    current = current.left;
  }
  return current;
}

class Node {
  constructor(key, value) {
    this.key = key;
    this.value = value;
    this.left = null;
    this.right = null;
    this.parent = null;
    this.color = 'red';
  }

  successor() {
    let x = this;
    if (x.right) {
      return treeMinimum(x.right);
    }

    let y = x.parent;
    while (y && x === y.right) {
      x = y;
      y = y.parent;
    }
    return y;
  }
}

class RedBlackTree {
  constructor() {
    this.root = null;
  }

  put(key, value) {
    const insertionResult = this.treeInsert(key, value);
    if (!insertionResult.isNewEntry) {
      return insertionResult.oldValue;
    }

    let x = insertionResult.newNode;
    let y = null;

    while (x !== this.root && x.parent.color === 'red') {
      if (x.parent === x.parent.parent.left) {
        y = x.parent.parent.right;
        if (y && y.color === 'red') {
          // Case 1
          x.parent.color = 'black';
          y.color = 'black';
          x.parent.parent.color = 'red';
          x = x.parent.parent;
        } else {
          if (x === x.parent.right) {
            // Case 2
            x = x.parent;
            this.leftRotate(x);
          }
          // Case 3
          x.parent.color = 'black';
          x.parent.parent.color = 'red';
          this.rightRotate(x.parent.parent);
        }
      } else {
        // Same as "then" clause with "right" and "left" exchanged.
        y = x.parent.parent.left;
        if (y && y.color === 'red') {
          // Case 1
          x.parent.color = 'black';
          y.color = 'black';
          x.parent.parent.color = 'red';
          x = x.parent.parent;
        } else {
          if (x === x.parent.left) {
            // Case 2
            x = x.parent;
            this.rightRotate(x);
          }
          // Case 3
          x.parent.color = 'black';
          x.parent.parent.color = 'red';
          this.leftRotate(x.parent.parent);
        }
      }
    }

    this.root.color = 'black';
    return null;
  }

  remove(key) {
    const z = this.findNode(key);
    if (!z) {
      return null;
    }

    // Y is the node to be unlinked from the tree.
    let y;
    if (!z.left || !z.right) {
      y = z;
    } else {
      y = z.successor();
    }

    // Y is guaranteed to be non-null at this point.
    let x;
    if (y.left) {
      x = y.left;
    } else {
      x = y.right;
    }

    // X is the child of y which might potentially replace y in the tree. X might be null at
    // this point.
    let xParent;
    if (x) {
      x.parent = y.parent;
      xParent = x.parent;
    } else {
      xParent = y.parent;
    }
    if (!y.parent) {
      this.root = x;
    } else if (y === y.parent.left) {
      y.parent.left = x;
    } else {
      y.parent.right = x;
    }

    if (y !== z) {
      if (y.color === 'black') {
        this.removeFixup(x, xParent);
      }

      y.parent = z.parent;
      y.color = z.color;
      y.left = z.left;
      y.right = z.right;

      if (z.left) {
        z.left.parent = y;
      }
      if (z.right) {
        z.right.parent = y;
      }
      if (z.parent) {
        if (z.parent.left === z) {
          z.parent.left = y;
        } else {
          z.parent.right = y;
        }
      } else {
        this.root = y;
      }
    } else if (y.color === 'black') {
      this.removeFixup(x, xParent);
    }

    return z.value;
  }

  get(key) {
    const node = this.findNode(key);
    if (!node) {
      return null;
    }
    return node.value;
  }

  forEach(callback) {
    if (!this.root) {
      return;
    }
    let current = treeMinimum(this.root);
    while (current) {
      callback(new Entry(current.key, current.value));
      current = current.successor();
    }
  }

  findNode(key) {
    for (let current = this.root; current;) {
      const comparisonResult = key.compareTo(current.key);
      if (!comparisonResult) {
        return current;
      }
      if (comparisonResult < 0) {
        current = current.left;
      } else {
        current = current.right;
      }
    }
    return null;
  }

  treeInsert(key, value) {
    let y = null;
    let x = this.root;
    while (x) {
      y = x;
      const comparisonResult = key.compareTo(x.key);
      if (comparisonResult < 0) {
        x = x.left;
      } else if (comparisonResult > 0) {
        x = x.right;
      } else {
        const oldValue = x.value;
        x.value = value;
        return new InsertResult(false, null, oldValue);
      }
    }

    const z = new Node(key, value);
    z.parent = y;
    if (!y) {
      this.root = z;
    } else if (key.compareTo(y.key) < 0) {
      y.left = z;
    } else {
      y.right = z;
    }
    return new InsertResult(true, z, null);
  }

  leftRotate(x) {
    const y = x.right;

    // Turn y's left subtree into x's right subtree.
    x.right = y.left;
    if (y.left) {
      y.left.parent = x;
    }

    // Link x's parent to y.
    y.parent = x.parent;
    if (!x.parent) {
      this.root = y;
    } else if (x === x.parent.left) {
      x.parent.left = y;
    } else {
      x.parent.right = y;
    }

    // Put x on y's left.
    y.left = x;
    x.parent = y;

    return y;
  }

  rightRotate(y) {
    const x = y.left;

    // Turn x's right subtree into y's left subtree.
    y.left = x.right;
    if (x.right) {
      x.right.parent = y;
    }

    // Link y's parent to x;
    x.parent = y.parent;
    if (!y.parent) {
      this.root = x;
    } else if (y === y.parent.left) {
      y.parent.left = x;
    } else {
      y.parent.right = x;
    }

    x.right = y;
    y.parent = x;

    return x;
  }

  removeFixup(x, xParent) {
    let w = null;
    while (x !== this.root && (!x || x.color === 'black')) {
      if (x === xParent.left) {
        // Note: the text points out that w cannot be null. The reason is not obvious from
        // simply looking at the code; it comes about from the properties of the red-black
        // tree.
        w = xParent.right;
        if (w.color === 'red') {
          // Case 1
          w.color = 'black';
          xParent.color = 'red';
          this.leftRotate(xParent);
          w = xParent.right;
        }
        if ((!w.left || w.left.color === 'black')
          && (!w.right || w.right.color === 'black')) {
          // Case 2
          w.color = 'red';
          x = xParent;
          xParent = x.parent;
        } else {
          if (!w.right || w.right.color === 'black') {
            // Case 3
            w.left.color = 'black';
            w.color = 'red';
            this.rightRotate(w);
            w = xParent.right;
          }
          // Case 4
          w.color = xParent.color;
          xParent.color = 'black';
          if (w.right) {
            w.right.color = 'black';
          }
          this.leftRotate(xParent);
          x = this.root;
          xParent = x.parent;
        }
      } else {
        // Same as "then" clause with "right" and "left" exchanged.
        w = xParent.left;
        if (w.color === 'red') {
          // Case 1
          w.color = 'black';
          xParent.color = 'red';
          this.rightRotate(xParent);
          w = xParent.left;
        }
        if ((!w.right || w.right.color === 'black')
          && (!w.left || w.left.color === 'black')) {
          // Case 2
          w.color = 'red';
          x = xParent;
          xParent = x.parent;
        } else {
          if (!w.left || w.left.color === 'black') {
            // Case 3
            w.right.color = 'black';
            w.color = 'red';
            this.leftRotate(w);
            w = xParent.left;
          }
          // Case 4
          w.color = xParent.color;
          xParent.color = 'black';
          if (w.left) {
            w.left.color = 'black';
          }
          this.rightRotate(xParent);
          x = this.root;
          xParent = x.parent;
        }
      }
    }
    if (x) {
      x.color = 'black';
    }
  }
}

class CallSign {
  constructor(value) {
    this.value = value;
  }

  compareTo(other) {
    return this.value === other.value ? 0 : this.value < other.value ? -1 : 1;
  }
}

class Collision {
  constructor(aircraftA, aircraftB, position) {
    this.aircraftA = aircraftA;
    this.aircraftB = aircraftB;
    this.position = position;
  }
}

const MIN_X = 0;
const MIN_Y = 0;
const MAX_X = 1000;
const MAX_Y = 1000;
const MIN_Z = 0;
const MAX_Z = 10;
const PROXIMITY_RADIUS = 1;
const GOOD_VOXEL_SIZE = PROXIMITY_RADIUS * 2;

function isInVoxel(voxel, motion) {
  if (voxel.x > MAX_X
    || voxel.x < MIN_X
    || voxel.y > MAX_Y
    || voxel.y < MIN_Y) {
    return false;
  }

  const init = motion.posOne;
  const fin = motion.posTwo;

  const vS = GOOD_VOXEL_SIZE;
  const r = PROXIMITY_RADIUS / 2;

  const vX = voxel.x;
  const x0 = init.x;
  const xv = fin.x - init.x;

  const vY = voxel.y;
  const y0 = init.y;
  const yv = fin.y - init.y;

  let lowX = (vX - r - x0) / xv;
  let highX = (vX + vS + r - x0) / xv;

  if (xv < 0) {
    const tmp = lowX;
    lowX = highX;
    highX = tmp;
  }

  let lowY = (vY - r - y0) / yv;
  let highY = (vY + vS + r - y0) / yv;

  if (yv < 0) {
    const tmp = lowY;
    lowY = highY;
    highY = tmp;
  }

  return (((xv === 0 && vX <= x0 + r && x0 - r <= vX + vS) /* no motion in x */
    || ((lowX <= 1 && 1 <= highX) || (lowX <= 0 && 0 <= highX)
    || (0 <= lowX && highX <= 1)))
    && ((yv === 0 && vY <= y0 + r && y0 - r <= vY + vS)
           || /* no motion in y */ ((lowY <= 1 && 1 <= highY) || (lowY <= 0 && 0 <= highY)
            || (0 <= lowY && highY <= 1)))
          && (xv === 0 || yv === 0 /* no motion in x or y or both */
           || (lowY <= highX && highX <= highY)
           || (lowY <= lowX && lowX <= highY)
           || (lowX <= lowY && highY <= highX)));
}

function putIntoMap(voxelMap, voxel, motion) {
  let vec = voxelMap.get(voxel);
  if (!vec) {
    vec = new som.Vector();
    voxelMap.put(voxel, vec);
  }
  vec.append(motion);
}

const horizontal = new Vector2D(GOOD_VOXEL_SIZE, 0);
const vertical = new Vector2D(0, GOOD_VOXEL_SIZE);

function recurse(voxelMap, seen, nextVoxel, motion) {
  if (!isInVoxel(nextVoxel, motion)) {
    return;
  }
  if (seen.put(nextVoxel, true)) {
    return;
  }

  putIntoMap(voxelMap, nextVoxel, motion);

  recurse(voxelMap, seen, nextVoxel.minus(horizontal), motion);
  recurse(voxelMap, seen, nextVoxel.plus(horizontal), motion);
  recurse(voxelMap, seen, nextVoxel.minus(vertical), motion);
  recurse(voxelMap, seen, nextVoxel.plus(vertical), motion);
  recurse(voxelMap, seen, nextVoxel.minus(horizontal).minus(vertical), motion);
  recurse(voxelMap, seen, nextVoxel.minus(horizontal).plus(vertical), motion);
  recurse(voxelMap, seen, nextVoxel.plus(horizontal).minus(vertical), motion);
  recurse(voxelMap, seen, nextVoxel.plus(horizontal).plus(vertical), motion);
}

function voxelHash(position) {
  // eslint-disable-next-line no-bitwise
  const xDiv = (position.x / GOOD_VOXEL_SIZE) | 0;
  // eslint-disable-next-line no-bitwise
  const yDiv = (position.y / GOOD_VOXEL_SIZE) | 0;

  const result = new Vector2D();
  result.x = GOOD_VOXEL_SIZE * xDiv;
  result.y = GOOD_VOXEL_SIZE * yDiv;

  if (position.x < 0) {
    result.x -= GOOD_VOXEL_SIZE;
  }

  if (position.y < 0) {
    result.y -= GOOD_VOXEL_SIZE;
  }

  return result;
}

function drawMotionOnVoxelMap(voxelMap, motion) {
  const seen = new RedBlackTree();
  recurse(voxelMap, seen, voxelHash(motion.posOne), motion);
}

function reduceCollisionSet(motions) {
  const voxelMap = new RedBlackTree();
  motions.forEach((motion) => drawMotionOnVoxelMap(voxelMap, motion));

  const result = new som.Vector();
  voxelMap.forEach((e) => {
    if (e.value.size() > 1) {
      result.append(e.value);
    }
  });
  return result;
}

class Motion {
  constructor(callsign, posOne, posTwo) {
    this.callsign = callsign;
    this.posOne = posOne;
    this.posTwo = posTwo;
  }

  delta() {
    return this.posTwo.minus(this.posOne);
  }

  findIntersection(other) {
    const init1 = this.posOne;
    const init2 = other.posOne;
    const vec1 = this.delta();
    const vec2 = other.delta();
    const radius = PROXIMITY_RADIUS;

    // this test is not geometrical 3-d intersection test, it takes the fact that the aircraft move
    // into account ; so it is more like a 4d test
    // (it assumes that both of the aircraft have a constant speed over the tested interval)

    // we thus have two points,
    // each of them moving on its line segment at constant speed ; we are looking
    // for times when the distance between these two points is smaller than r

    // vec1 is vector of aircraft 1
    // vec2 is vector of aircraft 2

    // a = (V2 - V1)^T * (V2 - V1)
    const a = vec2.minus(vec1).squaredMagnitude();

    if (a !== 0) {
      // we are first looking for instances of time when the planes are exactly r from each other
      // at least one plane is moving ; if the planes are moving in parallel,
      // they do not have constant speed

      // if the planes are moving in parallel, then
      //   if the faster starts behind the slower, we can have 2, 1, or 0 solutions
      //   if the faster plane starts in front of the slower, we can have 0 or 1 solutions

      // if the planes are not moving in parallel, then

      // point P1 = I1 + vV1
      // point P2 = I2 + vV2
      //   - looking for v, such that dist(P1,P2) = || P1 - P2 || = r

      // it follows that || P1 - P2 || = sqrt( < P1-P2, P1-P2 > )
      //   0 = -r^2 + < P1 - P2, P1 - P2 >
      //  from properties of dot product
      //   0 = -r^2 + <I1-I2,I1-I2> + v * 2<I1-I2, V1-V2> + v^2 *<V1-V2,V1-V2>
      //   so we calculate a, b, c - and solve the quadratic equation
      //   0 = c + bv + av^2

      // b = 2 * <I1-I2, V1-V2>
      const b = 2 * init1.minus(init2).dot(vec1.minus(vec2));

      // c = -r^2 + (I2 - I1)^T * (I2 - I1)
      const c = -radius * radius + init2.minus(init1).squaredMagnitude();

      const discr = b * b - 4 * a * c;
      if (discr < 0) {
        return null;
      }

      const v1 = (-b - Math.sqrt(discr)) / (2 * a);
      const v2 = (-b + Math.sqrt(discr)) / (2 * a);

      if (v1 <= v2 && ((v1 <= 1 && 1 <= v2)
                      || (v1 <= 0 && 0 <= v2)
                      || (0 <= v1 && v2 <= 1))) {
        // Pick a good "time" at which to report the collision.
        let v;
        if (v1 <= 0) {
          // The collision started before this frame. Report it at the start of the frame.
          v = 0;
        } else {
          // The collision started during this frame. Report it at that moment.
          v = v1;
        }

        const result1 = init1.plus(vec1.times(v));
        const result2 = init2.plus(vec2.times(v));

        const result = result1.plus(result2).times(0.5);
        if (result.x >= MIN_X
          && result.x <= MAX_X
          && result.y >= MIN_Y
          && result.y <= MAX_Y
          && result.z >= MIN_Z
          && result.z <= MAX_Z) {
          return result;
        }
      }

      return null;
    }

    // the planes have the same speeds and are moving in parallel (or they are not moving at all)
    // they  thus have the same distance all the time ; we calculate it from the initial point

    // dist = || i2 - i1 || = sqrt(  ( i2 - i1 )^T * ( i2 - i1 ) )
    const dist = init2.minus(init1).magnitude();
    if (dist <= radius) {
      return init1.plus(init2).times(0.5);
    }

    return null;
  }
}

class CollisionDetector {
  constructor() {
    this.state = new RedBlackTree();
  }

  handleNewFrame(frame) {
    const motions = new som.Vector();
    const seen = new RedBlackTree();

    frame.forEach((aircraft) => {
      let oldPosition = this.state.put(aircraft.callsign, aircraft.position);
      const newPosition = aircraft.position;
      seen.put(aircraft.callsign, true);

      if (!oldPosition) {
        // Treat newly introduced aircraft as if they were stationary.
        oldPosition = newPosition;
      }
      motions.append(new Motion(aircraft.callsign, oldPosition, newPosition));
    });

    // Remove aircraft that are no longer present.
    const toRemove = new som.Vector();
    this.state.forEach((e) => {
      if (!seen.get(e.key)) {
        toRemove.append(e.key);
      }
    });

    toRemove.forEach((e) => this.state.remove(e));

    const allReduced = reduceCollisionSet(motions);
    const collisions = new som.Vector();

    allReduced.forEach((reduced) => {
      for (let i = 0; i < reduced.size(); i += 1) {
        const motion1 = reduced.at(i);
        for (let j = i + 1; j < reduced.size(); j += 1) {
          const motion2 = reduced.at(j);
          const collision = motion1.findIntersection(motion2);
          if (collision) {
            collisions.append(new Collision(motion1.callsign, motion2.callsign, collision));
          }
        }
      }
    });

    return collisions;
  }
}
class Aircraft {
  constructor(callsign, position) {
    this.callsign = callsign;
    this.position = position;
  }
}

class Simulator {
  constructor(numAircraft) {
    this.aircraft = new som.Vector();
    for (let i = 0; i < numAircraft; i += 1) {
      this.aircraft.append(new CallSign(i));
    }
  }

  simulate(time) {
    const frame = new som.Vector();
    for (let i = 0; i < this.aircraft.size(); i += 2) {
      frame.append(new Aircraft(
        this.aircraft.at(i),
        new Vector3D(time, Math.cos(time) * 2 + i * 3, 10)
      ));
      frame.append(new Aircraft(
        this.aircraft.at(i + 1),
        new Vector3D(time, Math.sin(time) * 2 + i * 3, 10)
      ));
    }
    return frame;
  }
}

class CD extends Benchmark {
  cd(numAircrafts) {
    const numFrames = 200;
    const simulator = new Simulator(numAircrafts);
    const detector = new CollisionDetector();

    let actualCollisions = 0;
    for (let i = 0; i < numFrames; i += 1) {
      const time = i / 10;
      const collisions = detector.handleNewFrame(simulator.simulate(time));
      actualCollisions += collisions.size();
    }
    return actualCollisions;
  }

  verifyResult(actualCollisions, numAircrafts) {
    if (numAircrafts === 1000) { return actualCollisions === 14484; }
    if (numAircrafts === 500) { return actualCollisions === 14484; }
    if (numAircrafts === 250) { return actualCollisions === 10830; }
    if (numAircrafts === 200) { return actualCollisions === 8655; }
    if (numAircrafts === 100) { return actualCollisions === 4305; }
    if (numAircrafts === 10) { return actualCollisions === 390; }
    if (numAircrafts === 2) { return actualCollisions === 42; }

    process.stdout.write(`No verification result for ${numAircrafts} found`);
    process.stdout.write(`Result is: ${actualCollisions}`);
    return false;
  }

  innerBenchmarkLoop(innerIterations) {
    return this.verifyResult(this.cd(innerIterations), innerIterations);
  }
}

exports.newInstance = () => new CD();

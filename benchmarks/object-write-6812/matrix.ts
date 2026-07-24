// Scaled object-write coverage/rejection matrix for #6812.
//
// Every timed cell is deliberately at least about 100 ms on the development
// host. Setup and checksum work stay outside the timed region. Run one cell at
// a time:
//
//   node --experimental-strip-types matrix.ts key_dot
//   ./matrix key_dot

type CellResult = {
  elapsed: number;
  writes: number;
  sink: number;
};

function checksum(objects: any[], keys: string[]): number {
  let sink = 0;
  for (let i = 0; i < objects.length; i++) {
    const object: any = objects[i];
    for (let k = 0; k < keys.length; k++) {
      sink += object[keys[k]];
    }
  }
  return sink;
}

function keyDot(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function keyLiteral(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object["x"] = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function keyStableDynamic(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const key = "x";
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object[key] = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function keyAlternatingDynamic(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 10000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      const key = (i & 1) === 0 ? "x" : "y";
      object[key] = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 24000000, sink: checksum(objects, ["x", "y"]) };
}

function rhsNumeric(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function rhsPointer(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const pointer: any = { value: 17 };
  const t0 = Date.now();
  for (let r = 0; r < 40000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = pointer;
    }
  }
  const elapsed = Date.now() - t0;
  let sink = 0;
  for (let i = 0; i < objects.length; i++) {
    sink += objects[i].x.value;
  }
  return { elapsed, writes: 96000000, sink };
}

function rhsAllocating(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 8000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = { value: r + i };
    }
  }
  const elapsed = Date.now() - t0;
  let sink = 0;
  for (let i = 0; i < objects.length; i++) {
    sink += objects[i].x.value;
  }
  return { elapsed, writes: 19200000, sink };
}

function rhsCall(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const candidates: any[] = [(left: number, right: number) => left + right];
  const produce: any = candidates[0];
  const t0 = Date.now();
  for (let r = 0; r < 45000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = produce(r, i);
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 108000000, sink: checksum(objects, ["x"]) };
}

function shapeMonomorphic(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function shapeTwo(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push(
      (i & 1) === 0 ? { x: i, a: 0 } : { x: i, b: 0 },
    );
  }
  const t0 = Date.now();
  for (let r = 0; r < 40000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 96000000, sink: checksum(objects, ["x"]) };
}

function shapeFour(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    const kind = i & 3;
    if (kind === 0) objects.push({ x: i, a: 0 });
    else if (kind === 1) objects.push({ x: i, b: 0 });
    else if (kind === 2) objects.push({ x: i, c: 0 });
    else objects.push({ x: i, d: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 25000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 60000000, sink: checksum(objects, ["x"]) };
}

function shapeTransitionBeforeLoop(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  for (let i = 0; i < 2400; i++) {
    objects[i].added = i;
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function fieldsOne(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function fieldsTwo(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.z = r + i;
      object.w = r - i;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 240000000,
    sink: checksum(objects, ["z", "w"]),
  };
}

function fieldsFour(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 40000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
      object.y = r - i;
      object.z = r + i + 1;
      object.w = r - i - 1;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 384000000,
    sink: checksum(objects, ["x", "y", "z", "w"]),
  };
}

function fieldsEight(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ a: 0, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0, h: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 20000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.a = r + i;
      object.b = r - i;
      object.c = r + i + 1;
      object.d = r - i - 1;
      object.e = r + i + 2;
      object.f = r - i - 2;
      object.g = r + i + 3;
      object.h = r - i - 3;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 384000000,
    sink: checksum(objects, ["a", "b", "c", "d", "e", "f", "g", "h"]),
  };
}

function loopSingleCounted(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let n = 0; n < 120000000; n++) {
    const object: any = objects[n % 2400];
    object.x = n;
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function loopCurrentNested(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.z = r + i;
      object.w = r - i;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 240000000,
    sink: checksum(objects, ["z", "w"]),
  };
}

function loopStableLocalBounds(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const rounds = 40000;
  const length = 2400;
  const t0 = Date.now();
  for (let r = 0; r < rounds; r++) {
    for (let i = 0; i < length; i++) {
      const object: any = objects[i];
      object.z = r + i;
      object.w = r - i;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 192000000,
    sink: checksum(objects, ["z", "w"]),
  };
}

function loopNonzeroStart(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2405; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 40000; r++) {
    for (let i = 5; i < 2405; i++) {
      const object: any = objects[i];
      object.z = r + i;
      object.w = r - i;
    }
  }
  const elapsed = Date.now() - t0;
  return {
    elapsed,
    writes: 192000000,
    sink: checksum(objects, ["z", "w"]),
  };
}

function storageInline(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i, y: i * 2, z: 0, w: 0 });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function storageOverflow(): CellResult {
  const objects: any[] = [];
  const overflowKey = "x";
  for (let i = 0; i < 2400; i++) {
    const object: any = { a0: 0, a1: 0, a2: 0, a3: 0 };
    object[overflowKey] = i;
    objects.push(object);
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function receiverAnonymous(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push({ x: i });
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function receiverClassInstance(): CellResult {
  class Cell {
    x = 0;
  }
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push(new Cell());
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

function receiverClassIdZero(): CellResult {
  const objects: any[] = [];
  for (let i = 0; i < 2400; i++) {
    objects.push(JSON.parse('{"x":0}'));
  }
  const t0 = Date.now();
  for (let r = 0; r < 50000; r++) {
    for (let i = 0; i < 2400; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  const elapsed = Date.now() - t0;
  return { elapsed, writes: 120000000, sink: checksum(objects, ["x"]) };
}

const name = process.argv[2] || "key_dot";
let result: CellResult;
if (name === "key_dot") result = keyDot();
else if (name === "key_literal") result = keyLiteral();
else if (name === "key_stable_dynamic") result = keyStableDynamic();
else if (name === "key_alternating_dynamic") result = keyAlternatingDynamic();
else if (name === "rhs_numeric") result = rhsNumeric();
else if (name === "rhs_pointer") result = rhsPointer();
else if (name === "rhs_allocating") result = rhsAllocating();
else if (name === "rhs_call") result = rhsCall();
else if (name === "shape_monomorphic") result = shapeMonomorphic();
else if (name === "shape_two") result = shapeTwo();
else if (name === "shape_four") result = shapeFour();
else if (name === "shape_transition_before_loop") {
  result = shapeTransitionBeforeLoop();
} else if (name === "fields_one") result = fieldsOne();
else if (name === "fields_two") result = fieldsTwo();
else if (name === "fields_four") result = fieldsFour();
else if (name === "fields_eight") result = fieldsEight();
else if (name === "loop_single_counted") result = loopSingleCounted();
else if (name === "loop_current_nested") result = loopCurrentNested();
else if (name === "loop_stable_local_bounds") result = loopStableLocalBounds();
else if (name === "loop_nonzero_start") result = loopNonzeroStart();
else if (name === "storage_inline") result = storageInline();
else if (name === "storage_overflow") result = storageOverflow();
else if (name === "receiver_anonymous") result = receiverAnonymous();
else if (name === "receiver_class_instance") result = receiverClassInstance();
else if (name === "receiver_class_id_zero") result = receiverClassIdZero();
else throw new Error(`unknown matrix cell: ${name}`);

console.log(
  "cell",
  name,
  "ms",
  result.elapsed,
  "writes",
  result.writes,
  "sink",
  result.sink,
);

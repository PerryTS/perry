// Promise resolution reads `then` off the resolution value
// (ECMA-262 27.2.1.3.2). The read must follow every change to the value's
// own keys and prototype chain, for receivers of one shape seen repeatedly.

const log: string[] = [];

class Plain {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

function Ctor(this: any, v: number) {
  this.v = v;
}

async function run(): Promise<void> {
  // 1. Class instances and constructor instances: not thenables.
  let sum = 0;
  for (let i = 0; i < 50; i++) {
    const a = await Promise.resolve(new Plain(i));
    const b: any = await Promise.resolve(new (Ctor as any)(i));
    sum += a.v + b.v;
  }
  log.push("plain " + sum);

  // 2. A prototype `then`, then replaced, then deleted.
  const proto: any = {
    then(res: (v: string) => void) {
      res("proto-a");
    },
  };
  const make = () => Object.create(proto);
  for (let i = 0; i < 3; i++) log.push("p1 " + (await make()));
  proto.then = (res: (v: string) => void) => res("proto-b");
  for (let i = 0; i < 3; i++) log.push("p2 " + (await make()));
  delete proto.then;
  for (let i = 0; i < 3; i++) {
    const o = make();
    log.push("p3 " + ((await o) === o));
  }

  // 3. An own `then` on one receiver of the shared shape.
  const own = make();
  own.then = (res: (v: string) => void) => res("own");
  log.push("own " + (await own));
  log.push("other " + ((await make()) !== undefined));

  // 4. setPrototypeOf to a thenable prototype, and to null.
  const moved: any = make();
  Object.setPrototypeOf(moved, {
    then(res: (v: string) => void) {
      res("moved");
    },
  });
  log.push("moved " + (await moved));
  const bare: any = make();
  Object.setPrototypeOf(bare, null);
  log.push("null " + ((await bare) === bare));

  // 5. A throwing `then` getter on the prototype rejects.
  const thrower: any = Object.create(
    Object.defineProperty({}, "then", {
      get() {
        throw new Error("boom");
      },
    }),
  );
  try {
    await thrower;
    log.push("no throw");
  } catch (e: any) {
    log.push("caught " + e.message);
  }

  // 6. A class whose prototype gains `then` later.
  class Later {
    x = 1;
  }
  for (let i = 0; i < 3; i++) log.push("l1 " + ((await new Later()) instanceof Later));
  (Later.prototype as any).then = (res: (v: string) => void) => res("later");
  log.push("l2 " + (await new Later()));
}

run().then(() => console.log(log.join("\n")));

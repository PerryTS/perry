// A borrowed mutator must refresh its remaining arguments after a trap moves
// them. Exercise both the small native buffer and lists wider than 16 slots.
const push: any = Array.prototype.push;
function run(count: number) {
  // Straight-line births stay young until a trap forces the first poll.
  const items = count === 2 ? [{ tag: 100 }, { tag: 101 }] : [
    { tag: 100 },
    { tag: 101 },
    { tag: 102 },
    { tag: 103 },
    { tag: 104 },
    { tag: 105 },
    { tag: 106 },
    { tag: 107 },
    { tag: 108 },
    { tag: 109 },
    { tag: 110 },
    { tag: 111 },
    { tag: 112 },
    { tag: 113 },
    { tag: 114 },
    { tag: 115 },
    { tag: 116 },
    { tag: 117 },
    { tag: 118 },
    { tag: 119 },
  ];
  const target: any = { length: 0 };
  const proxy = new Proxy(target, {
    set(obj, key, value) {
      const garbage: any[] = [];
      for (let i = 0; i < 40; i++) garbage.push({ n: i });
      if (garbage.length !== 40) throw new Error("churn");
      return Reflect.set(obj, key, value);
    },
  });
  Reflect.apply(push, proxy, items);
  let sum = 0;
  for (let i = 0; i < count; i++) sum += target[i].tag;
  console.log(target.length, sum, target[count - 1] === items[count - 1]);
}
run(2);
run(20);

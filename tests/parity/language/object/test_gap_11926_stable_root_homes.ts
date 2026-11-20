// #11926: a root-heavy function uses one stable precise frame home per live
// object. An evacuating gc() between construction and use must rewrite those
// homes, and every property read below must reload the moved value.
declare function gc(): void;
function sink(x: any): any {
  return x;
}

function big(seed: unknown): number {
  let v0: any = sink({ k: seed, i: 0 });
  let v1: any = sink({ k: seed, i: 1 });
  let v2: any = sink({ k: seed, i: 2 });
  let v3: any = sink({ k: seed, i: 3 });
  let v4: any = sink({ k: seed, i: 4 });
  let v5: any = sink({ k: seed, i: 5 });
  let v6: any = sink({ k: seed, i: 6 });
  let v7: any = sink({ k: seed, i: 7 });
  let v8: any = sink({ k: seed, i: 8 });
  let v9: any = sink({ k: seed, i: 9 });
  let v10: any = sink({ k: seed, i: 10 });
  let v11: any = sink({ k: seed, i: 11 });
  let v12: any = sink({ k: seed, i: 12 });
  let v13: any = sink({ k: seed, i: 13 });
  let v14: any = sink({ k: seed, i: 14 });
  let v15: any = sink({ k: seed, i: 15 });
  let v16: any = sink({ k: seed, i: 16 });
  let v17: any = sink({ k: seed, i: 17 });
  let v18: any = sink({ k: seed, i: 18 });
  let v19: any = sink({ k: seed, i: 19 });
  let v20: any = sink({ k: seed, i: 20 });
  let v21: any = sink({ k: seed, i: 21 });
  let v22: any = sink({ k: seed, i: 22 });
  let v23: any = sink({ k: seed, i: 23 });
  let v24: any = sink({ k: seed, i: 24 });

  sink(v0);
  sink(v1);
  sink(v2);
  sink(v3);
  sink(v4);
  sink(v5);
  sink(v6);
  sink(v7);
  sink(v8);
  sink(v9);
  sink(v10);
  sink(v11);
  sink(v12);
  sink(v13);
  sink(v14);
  sink(v15);
  sink(v16);
  sink(v17);
  sink(v18);
  sink(v19);
  sink(v20);
  sink(v21);
  sink(v22);
  sink(v23);
  sink(v24);

  if (typeof gc === "function") gc();

  return (
    v0.i +
    v1.i +
    v2.i +
    v3.i +
    v4.i +
    v5.i +
    v6.i +
    v7.i +
    v8.i +
    v9.i +
    v10.i +
    v11.i +
    v12.i +
    v13.i +
    v14.i +
    v15.i +
    v16.i +
    v17.i +
    v18.i +
    v19.i +
    v20.i +
    v21.i +
    v22.i +
    v23.i +
    v24.i
  );
}

console.log(big("seed"));

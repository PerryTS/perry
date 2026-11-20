// Closed World: a function-scope var assigned once before the closures that
// read it can run is proven; a twin whose reader runs first is not.
function makeTable(): (i: number) => any {
  var table: any = (function () {
    var a: any = [];
    for (var i = 0; i < 8; i++) a.push("t" + i);
    return a;
  })();
  return function (i: number): any {
    return table[i];
  };
}

function makeEarly(): any[] {
  const seen: any[] = [];
  function look(): any {
    const out: any[] = [];
    out.push(table ? "truthy" : "falsy");
    out.push(String(Array.isArray(table)));
    try { out.push(String(table.length)); } catch (e) { out.push((e as Error).constructor.name); }
    try { out.push(table.join("-")); } catch (e) { out.push((e as Error).constructor.name); }
    return out.join("/");
  }
  seen.push(look());
  var table: any = (function () {
    var a: any = [];
    a.push("x");
    return a;
  })();
  seen.push(look());
  return seen;
}

const t = makeTable();
console.log(t(0), t(7), String(t(8)));
const t2 = makeTable();
console.log(t2(3), t === t2);
console.log(makeEarly().join(","));

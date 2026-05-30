function keyForLabel(sym: any): string {
  try {
    const key = Symbol.keyFor(sym);
    return key === undefined ? "undefined" : key;
  } catch (err: any) {
    return err?.name ?? "throw";
  }
}

function descriptionLabel(sym: any): string {
  if (typeof sym !== "symbol") {
    return "not-symbol";
  }
  return sym.description ?? "undefined";
}

function show(name: string, sym: any) {
  console.log(name, typeof sym, String(sym), descriptionLabel(sym), keyForLabel(sym));
}

show("species", Symbol.species);
show("match", Symbol.match);
show("matchAll", Symbol.matchAll);
show("replace", Symbol.replace);
show("search", Symbol.search);
show("split", Symbol.split);
show("isConcatSpreadable", Symbol.isConcatSpreadable);
show("unscopables", Symbol.unscopables);

console.log("species stable", Symbol.species === Symbol.species);
console.log("match distinct registry", Symbol.match === Symbol.for("Symbol.match"));

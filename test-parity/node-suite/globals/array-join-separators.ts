function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ": " + JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label + ": throws " + err.name + ": " + err.message);
  }
}

const separatorObject = {
  toString() {
    return "|";
  },
};

const dynamicJoin = (array: any, separator?: any) => array["join"](separator);

show("static omitted", () => [1, 2].join());
show("static undefined", () => [1, 2].join(undefined));
show("static null", () => [1, 2].join(null as any));
show("static number", () => [1, 2].join(0 as any));
show("static short string", () => [1, 2].join("|"));
show("static heap string", () => [1, 2].join("separator"));
show("static object", () => [1, 2].join(separatorObject as any));
show("dynamic undefined", () => dynamicJoin([1, 2]));
show("dynamic null", () => dynamicJoin([1, 2], null));
show("dynamic number", () => dynamicJoin([1, 2], 0));
show("dynamic object", () => dynamicJoin([1, 2], separatorObject));
show("nullish elements", () => [1, null, undefined, "x"].join("-"));

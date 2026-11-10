function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

const values: any = ["a"];
const value: any = values[0];

show("dynamic zero", value.concat());
show("dynamic multi", value.concat("b", "c"));
show("dynamic coerced", value.concat(2, true, null, undefined));

show("typed multi", "a".concat("b", "c"));

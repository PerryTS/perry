function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

function expectThrowName(label: string, fn: () => any) {
  try {
    fn();
    show(label, "no throw");
  } catch (err: any) {
    show(label, err.name);
  }
}

const sparse: any[] = [1, , 3];

const forEachCalls: string[] = [];
sparse.forEach((value, index, array) => {
  forEachCalls.push(index + ":" + String(value) + ":" + String(array === sparse));
});
show("forEach calls", forEachCalls.join("|"));

const mapCalls: string[] = [];
const mapped = sparse.map((value, index, array) => {
  mapCalls.push(index + ":" + String(value) + ":" + String(array === sparse));
  return value * 10;
});
show("map calls", mapCalls.join("|"));
show("map length", mapped.length);
show("map has1", 1 in mapped);
show("map join", mapped.join("|"));

const filterCalls: string[] = [];
const filtered = sparse.filter((value, index) => {
  filterCalls.push(index + ":" + String(value));
  return true;
});
show("filter calls", filterCalls.join("|"));
show("filter join", filtered.join("|"));

const someCalls: string[] = [];
show("some result", sparse.some((value, index) => {
  someCalls.push(index + ":" + String(value));
  return index === 1;
}));
show("some calls", someCalls.join("|"));

const everyCalls: string[] = [];
show("every result", sparse.every((value, index) => {
  everyCalls.push(index + ":" + String(value));
  return index !== 1;
}));
show("every calls", everyCalls.join("|"));

const flatMapCalls: string[] = [];
const flatMapped = sparse.flatMap((value, index) => {
  flatMapCalls.push(index + ":" + String(value));
  return [value, , value + 10];
});
show("flatMap calls", flatMapCalls.join("|"));
show("flatMap length", flatMapped.length);
show("flatMap join", flatMapped.join("|"));

const reduceCalls: string[] = [];
const reduced = [, 2, , 4].reduce((acc, value, index) => {
  reduceCalls.push(index + ":" + String(value));
  return String(acc) + "|" + index + ":" + String(value);
});
show("reduce result", reduced);
show("reduce calls", reduceCalls.join("|"));

const reduceInitCalls: string[] = [];
const reducedInit = [, 2, , 4].reduce((acc, value, index) => {
  reduceInitCalls.push(index + ":" + String(value));
  return String(acc) + "|" + index + ":" + String(value);
}, "init");
show("reduce init result", reducedInit);
show("reduce init calls", reduceInitCalls.join("|"));

const reduceRightCalls: string[] = [];
const reducedRight = [1, , 3, ,].reduceRight((acc, value, index) => {
  reduceRightCalls.push(index + ":" + String(value));
  return String(acc) + "|" + index + ":" + String(value);
});
show("reduceRight result", reducedRight);
show("reduceRight calls", reduceRightCalls.join("|"));

const holes = new Array(2);
let reduceHoleCalls = 0;
show("reduce holes init", holes.reduce((acc: string) => {
  reduceHoleCalls++;
  return acc;
}, "seed"));
show("reduce holes init calls", reduceHoleCalls);
expectThrowName("reduce holes no init", () => holes.reduce((acc: any, value: any) => acc || value));

const findCalls: string[] = [];
const found = sparse.find((value, index) => {
  findCalls.push(index + ":" + String(value));
  return index === 1;
});
show("find found undefined", found === undefined);
show("find calls", findCalls.join("|"));

const findIndexCalls: string[] = [];
show("findIndex hole", sparse.findIndex((value, index) => {
  findIndexCalls.push(index + ":" + String(value));
  return value === undefined;
}));
show("findIndex calls", findIndexCalls.join("|"));

const findLastCalls: string[] = [];
const foundLast = sparse.findLast((value, index) => {
  findLastCalls.push(index + ":" + String(value));
  return value === undefined;
});
show("findLast found undefined", foundLast === undefined);
show("findLast calls", findLastCalls.join("|"));

const findLastIndexCalls: string[] = [];
show("findLastIndex hole", sparse.findLastIndex((value, index) => {
  findLastIndexCalls.push(index + ":" + String(value));
  return value === undefined;
}));
show("findLastIndex calls", findLastIndexCalls.join("|"));

show("indexOf undefined", sparse.indexOf(undefined));
show("lastIndexOf undefined", sparse.lastIndexOf(undefined));
show("includes undefined", sparse.includes(undefined));
show("holes includes undefined", new Array(2).includes(undefined));

let emptyFindCalls = 0;
show("empty find undefined", [].find(() => {
  emptyFindCalls++;
  return true;
}) === undefined);
show("empty find calls", emptyFindCalls);

const deleted: any[] = [1, 2, 3];
delete deleted[1];
const deletedCalls: string[] = [];
deleted.map((value, index) => {
  deletedCalls.push(index + ":" + String(value));
  return value;
});
show("delete has1", 1 in deleted);
show("delete map calls", deletedCalls.join("|"));

const extended: any[] = [1];
extended.length = 3;
const extendedCalls: string[] = [];
extended.forEach((value, index) => {
  extendedCalls.push(index + ":" + String(value));
});
show("length extend has1", 1 in extended);
show("length extend calls", extendedCalls.join("|"));
show("at hole undefined", extended.at(1) === undefined);

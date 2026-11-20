function show(label: string, value: unknown) {
  console.log(label + ": " + String(value));
}

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    show(label, "no-throw");
  } catch (err: any) {
    show(label, err?.name + ":" + err?.message);
  }
}

const iter = "a1b2".matchAll(/(\d)/g);
show("iter next type", typeof (iter as any).next);
show("iter tag", Object.prototype.toString.call(iter));
show("iter self", (iter as any)[Symbol.iterator]() === iter);

const first = (iter as any).next();
show("manual first done", first.done);
show(
  "manual first value",
  [
    first.value[0],
    first.value[1],
    first.value.index,
    first.value.input,
    first.value.groups === undefined,
  ].join("|"),
);
show("manual rest", JSON.stringify(Array.from(iter, (m) => [m[0], m[1], m.index])));

showError("non-global", () => "a".matchAll(/a/));
show("string pattern", JSON.stringify(Array.from("a.a".matchAll("."), (m) => [m[0], m.index])));
show(
  "undefined pattern",
  JSON.stringify(Array.from("ab".matchAll(undefined), (m) => [m[0], m.index])),
);

const startRe = /a/g;
startRe.lastIndex = 2;
show("lastIndex seen", Array.from("a_a".matchAll(startRe), (m) => m.index).join(","));
show("lastIndex after", startRe.lastIndex);

const named = Array.from("xy".matchAll(/(?<letter>[xy])/g))[0];
show("named groups", [named[0], named.groups?.letter, named.index, named.input].join("|"));

const dynamicInput: any = "z9";
const dynamicIter = dynamicInput.matchAll(/(\d)/g);
const dynamicFirst = dynamicIter.next().value;
show("dynamic dispatch", [dynamicFirst[0], dynamicFirst[1], dynamicFirst.index].join("|"));

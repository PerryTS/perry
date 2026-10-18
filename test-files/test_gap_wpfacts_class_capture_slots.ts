// Class capture indices retain the class, array and descriptor independently.
class SlotBaseA { read() { return "A"; } }
class SlotBaseB { padding = 1; read() { return "B"; } }
function slots(Parent: any, values: number[], descriptor: { tag: string }) {
  return class extends Parent {
    static marker = 1;
    text() { return super.read() + ":" + values.join(",") + ":" + descriptor.tag; }
    static parent() { return new Parent().read(); }
  };
}
const SlotFirst = slots(SlotBaseA, [1, 2], { tag: "first" });
const SlotSecond = slots(SlotBaseB, [3], { tag: "second" });
console.log(new SlotFirst().text(), new SlotSecond().text());
console.log(SlotFirst.parent(), SlotSecond.parent());
function refreshedSlots() {
  let Parent: any = SlotBaseA;
  let values = ["old"];
  const descriptor = { tag: "kept" };
  class Holder {
    read() { return new Parent().read() + ":" + values[0] + ":" + descriptor.tag; }
  }
  Parent = SlotBaseB;
  values = ["new"];
  return new Holder();
}
console.log(refreshedSlots().read());
const slotActions = [(arg: { value: number }) => arg.value];
console.log("numeric-call", slotActions[0]({ value: 7 }));
function invalidSlotParent(valid: boolean) {
  return class extends (valid ? SlotBaseA : ({ notConstructor: true } as any)) { static marker = 1; };
}
const ValidSlotParent = invalidSlotParent(true);
console.log("valid-parent", new ValidSlotParent().read());
try { invalidSlotParent(false); console.log("invalid-parent", false); }
catch (error) { console.log("invalid-parent", error instanceof TypeError); }

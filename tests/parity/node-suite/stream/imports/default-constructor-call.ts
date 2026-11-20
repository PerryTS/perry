import Stream from "node:stream";

let touched = 0;
const receiver = {
  get marker() {
    touched++;
    return "receiver";
  },
};

const result = Stream.call(receiver);

console.log("result:", result === undefined);
console.log("own keys:", Object.keys(receiver).join(",") || "(none)");
console.log("getter side effect:", touched);
console.log("marker:", receiver.marker);

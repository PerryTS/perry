import { StringDecoder } from "node:string_decoder";

const cases = [
  ["dollar", Buffer.from("$", "utf8")],
  ["cent", Buffer.from("¢", "utf8")],
  ["euro", Buffer.from("€", "utf8")],
  ["supplementary", Buffer.from("𤭢", "utf8")],
] as const;

for (const [name, input] of cases) {
  const dec = new StringDecoder("utf8");
  console.log(name + ":", dec.write(input) + dec.end());
}

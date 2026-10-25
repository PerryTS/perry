import * as crypto from "node:crypto";

const dh = crypto.createDiffieHellman(512);
console.log("dh verifyError:", (dh as any).verifyError);
console.log("dh verifyError own/in:", "verifyError" in dh);
const group = crypto.createDiffieHellmanGroup("modp5");
console.log("group verifyError type:", typeof (group as any).verifyError);
console.log("group verifyError own/in:", "verifyError" in group);

function observePresence(label: string, receiver: any) {
  const key = ["verify", "Error"].join("");
  console.log(label, "dynamic verifyError:", receiver[key]);
  console.log(label, "dynamic verifyError in:", key in receiver);
  console.log(label, "verifyError Reflect.has:", Reflect.has(receiver, key));
  console.log(label, "generateKeys in:", "generateKeys" in receiver);
  console.log(label, "missing in:", "__dh_absent__" in receiver);
}
observePresence("dh", dh);
observePresence("group", group);

const units = ["a", "é", "東京", "🙂", "\n\"\\", "\ud800"];
const lengths = [3, 63, 64, 65, 127, 128, 4096];
const kept: any[] = [];
let hash = 0;
let failures = 0;
for (let round = 0; round < 8; round++) {
  for (let u = 0; u < units.length; u++) {
    for (let n = 0; n < lengths.length; n++) {
      const text = units[u].repeat(lengths[n]) + ":" + round;
      const source = '{"id":1,"text":' + JSON.stringify(text) + '}';
      const object = JSON.parse(source);
      const output = JSON.stringify(object);
      if (output !== source || JSON.parse(output).text !== text) failures++;
      for (let i = 0; i < output.length; i++) hash = (hash * 33 + output.charCodeAt(i)) % 1000000007;
      kept.push(object);
      kept.push(output);
    }
  }
}
for (let i = 0; i < kept.length; i += 2) {
  if (JSON.stringify(kept[i]) !== kept[i + 1]) failures++;
}
console.log("direct objects", kept.length, failures, hash);

let calls = "";
const getter = JSON.parse('{"id":1,"text":"plain"}');
Object.defineProperty(getter, "text", { enumerable: true, get() { calls += "get,"; return "observed"; } });
console.log(JSON.stringify(getter), calls);
const hidden = JSON.parse('{"visible":1,"hidden":2}');
Object.defineProperty(hidden, "hidden", { enumerable: false });
console.log(JSON.stringify(hidden));
const own = JSON.parse('{"id":1}');
own.toJSON = function(key: string) { calls += "own:" + key + ","; return "own-result"; };
console.log(JSON.stringify(own));
const proto = JSON.parse('{"id":1,"text":"plain"}');
Object.setPrototypeOf(proto, { toJSON(key: string) { calls += "proto:" + key + ","; return "proto-result"; } });
console.log(JSON.stringify(proto));

const fresh = JSON.parse('{"id":1,"text":"plain"}');
console.log(JSON.stringify(fresh));
(Object.prototype as any).toJSON = function(key: string) { calls += "default:" + key + ","; return "default-result"; };
console.log(JSON.stringify(fresh));
delete (Object.prototype as any).toJSON;
console.log(JSON.stringify(fresh), calls);
console.log(JSON.stringify(JSON.parse('{"2":2,"1":1,"a":0}')));
console.log(JSON.stringify(JSON.parse('{"a":1,"b":2,"c":3,"d":4,"e":5,"f":6}')));
console.log(JSON.stringify({ a: new Number(3), b: /x/, c: undefined }));

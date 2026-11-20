import * as util from "node:util";
function encodeLength(content: string) {
  content = textEncoder.encode(content) as any;
  return (content as any).byteLength;
}
var textEncoder = new util.TextEncoder();
var nestedLen = (() => {
  function encodeLengthNested(content: string) {
    content = nestedTextEncoder.encode(content) as any;
    return (content as any).byteLength;
  }
  var nestedTextEncoder = new util.TextEncoder();
  return encodeLengthNested("world");
})();
console.log("len=" + encodeLength("hello") + ",nested=" + nestedLen);

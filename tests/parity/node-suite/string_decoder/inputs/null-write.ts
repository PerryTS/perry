import { StringDecoder } from "node:string_decoder";

try {
  new StringDecoder("utf8").write(null as any);
  console.log("null ok");
} catch (e) {
  const err = e as any;
  console.log(err.name, err.message.startsWith('The "buf" argument must be an instance of Buffer, TypedArray, or DataView.'));
}

import { StringDecoder } from "node:string_decoder";

for (const enc of ["definitely-bad"] as any[]) {
  try {
    new StringDecoder(enc);
    console.log(String(enc), "ok");
  } catch (e) {
    const err = e as any;
    console.log(String(enc), err.name, err.message);
  }
}

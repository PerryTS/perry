import { Buffer } from "node:buffer";
import { getUnpackedSettings } from "node:http2";

for (const length of [1, 5, 7]) {
  try {
    getUnpackedSettings(Buffer.alloc(length));
  } catch (error: any) {
    console.log(length, error.name, error.code);
  }
}

import { Buffer } from "node:buffer";
import { getUnpackedSettings } from "node:http2";

const packed = Buffer.from("000500000001", "hex");
for (const validate of [false, true]) {
  try {
    console.log(
      validate,
      getUnpackedSettings(packed, { validate }).maxFrameSize,
    );
  } catch (error: any) {
    console.log(validate, error.name, error.code);
  }
}

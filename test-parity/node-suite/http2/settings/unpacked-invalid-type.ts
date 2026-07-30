import { getUnpackedSettings } from "node:http2";

for (const value of [1, true, "", [], {}, null]) {
  try {
    getUnpackedSettings(value as any);
  } catch (error: any) {
    console.log(error.name, error.code);
  }
}

import { Buffer } from "node:buffer";
import { getUnpackedSettings } from "node:http2";

const settings = getUnpackedSettings(Buffer.from("000600000064", "hex"));
console.log(settings.maxHeaderSize, settings.maxHeaderListSize);

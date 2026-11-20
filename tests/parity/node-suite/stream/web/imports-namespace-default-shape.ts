import streamWebDefault, {
  ByteLengthQueuingStrategy,
  CountQueuingStrategy,
  ReadableStream,
  TextDecoderStream,
  TextEncoderStream,
  TransformStream,
  WritableStream,
} from "node:stream/web";
import * as streamWeb from "node:stream/web";

const expected = [
  "ReadableStream",
  "WritableStream",
  "TransformStream",
  "ByteLengthQueuingStrategy",
  "CountQueuingStrategy",
  "TextEncoderStream",
  "TextDecoderStream",
] as const;

console.log("namespace default type:", typeof streamWeb.default);
console.log("default import identity:", streamWebDefault === streamWeb.default);
console.log("default is namespace:", streamWebDefault === streamWeb);
console.log(
  "default has own default:",
  Object.prototype.hasOwnProperty.call(streamWebDefault, "default"),
);

for (const name of expected) {
  console.log(
    `${name}:`,
    typeof streamWeb[name],
    typeof streamWebDefault[name],
    streamWeb[name] === streamWebDefault[name],
  );
}

console.log(
  "named identities:",
  ReadableStream === streamWeb.ReadableStream,
  WritableStream === streamWeb.WritableStream,
  TransformStream === streamWeb.TransformStream,
);
console.log(
  "strategy identities:",
  ByteLengthQueuingStrategy === streamWeb.ByteLengthQueuingStrategy,
  CountQueuingStrategy === streamWeb.CountQueuingStrategy,
);
console.log(
  "text stream identities:",
  TextEncoderStream === streamWeb.TextEncoderStream,
  TextDecoderStream === streamWeb.TextDecoderStream,
);

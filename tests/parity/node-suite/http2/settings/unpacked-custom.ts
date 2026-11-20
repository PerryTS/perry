import { Buffer } from "node:buffer";
import { getUnpackedSettings } from "node:http2";

function show(label: string, hex: string) {
  const settings = getUnpackedSettings(Buffer.from(hex, "hex"));
  console.log(label, JSON.stringify(settings));
  console.log(label + " keys", Object.keys(settings).join(","));
  if (settings.customSettings) {
    console.log(
      label + " custom keys",
      Object.keys(settings.customSettings).join(","),
    );
  }
}

show("single", "270f0000012d");
// Duplicate unknown IDs overwrite values; numeric keys sort even out of wire order.
show(
  "mixed",
  "00010000002a270f0000012d000800000001ffffffffffff00090000000a000700000001000000000002270f00000003ffff000000000006ffffffff",
);
show("unknown-first", "ffffffffffff00010000002a000700000000");
show("unknown-last", "00010000002a270f0000012d");
show("empty", "");
// Known IDs retain their names, aliases and nonzero boolean decoding.
show(
  "known",
  "0001000010000002000000020003ffffffff00040000ffff00050000000100060000002a000800000002",
);

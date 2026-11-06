import { getPackedSettings } from "node:http2";

const warnings: string[] = [];
const onWarning = (warning: Error) => warnings.push(warning.name);
process.on("warning", onWarning);
try {
  console.log(
    getPackedSettings({ maxHeaderSize: 1, maxHeaderListSize: 2 }).toString(
      "hex",
    ),
  );
  await new Promise<void>((resolve) => process.nextTick(resolve));
  console.log("warnings:", warnings.join(","));
} finally {
  process.off("warning", onWarning);
}

import { getPackedSettings } from "node:http2";

console.log(getPackedSettings({ unknown: 1 } as any).length);

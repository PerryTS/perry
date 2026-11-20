import { getPackedSettings } from "node:http2";

for (const key of ["enablePush", "enableConnectProtocol"] as const) {
  for (const value of [0, 1, null, "true"]) {
    try {
      getPackedSettings({ [key]: value as any });
    } catch (error: any) {
      console.log(key, String(value), error.name, error.code);
    }
  }
}

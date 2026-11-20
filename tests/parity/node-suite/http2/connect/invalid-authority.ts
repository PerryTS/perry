import { connect } from "node:http2";

for (const value of [1, null]) {
  try {
    connect(value as any);
  } catch (error: any) {
    console.log(String(value), error.name, error.code);
  }
}

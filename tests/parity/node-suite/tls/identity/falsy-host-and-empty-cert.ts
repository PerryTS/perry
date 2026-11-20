import tls from "node:tls";

for (
  const [label, host] of [["false", false], ["null", null], [
    "undefined",
    undefined,
  ]] as const
) {
  const cert = { subject: { CN: "a.example" } } as any;
  const err: any = tls.checkServerIdentity(host as any, cert);
  console.log(
    label + ":",
    err instanceof Error,
    err?.code,
    err?.host === String(host),
    err?.cert === cert,
  );
}
for (
  const [label, cert] of [["empty", {}], ["empty subject", {
    subject: {},
  }]] as const
) {
  const err: any = tls.checkServerIdentity("a.example", cert as any);
  console.log(
    label + ":",
    err instanceof Error,
    err?.code,
    err?.cert === cert,
    typeof err?.reason,
  );
}

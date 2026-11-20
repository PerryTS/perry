import { report as namedReport } from "node:process";

function line(name: string, value: unknown) {
  console.log(name + ": " + String(value));
}

function errorLine(name: string, fn: () => unknown) {
  try {
    fn();
  } catch (err: any) {
    line(name, err.name + ":" + err.code);
  }
}

// process.report is the diagnostic-report controller object.
line("is object", typeof process.report === "object" && process.report !== null);
line("named identity", namedReport === process.report);
line(
  "method shape",
  [
    typeof process.report.getReport,
    process.report.getReport.length,
    typeof process.report.writeReport,
    process.report.writeReport.length,
  ].join(","),
);

const report = process.report.getReport();

line(
  "top sections",
  [
    "header",
    "javascriptStack",
    "javascriptHeap",
    "nativeStack",
    "resourceUsage",
    "uvthreadResourceUsage",
    "libuv",
    "workers",
    "environmentVariables",
    "userLimits",
    "sharedObjects",
  ].map((key) => key + ":" + typeof (report as any)[key] + ":" + Array.isArray((report as any)[key])).join("|"),
);
line(
  "header core",
  [
    report.header.event,
    report.header.trigger,
    typeof report.header.reportVersion,
    typeof report.header.processId,
    Array.isArray(report.header.commandLine),
    typeof report.header.nodejsVersion,
    typeof report.header.componentVersions,
    typeof report.header.release,
  ].join(","),
);
line(
  "stack core",
  [
    typeof report.javascriptStack.message,
    Array.isArray(report.javascriptStack.stack),
    typeof report.javascriptStack.errorProperties,
  ].join(","),
);
line(
  "heap resource core",
  [
    typeof report.javascriptHeap.totalMemory,
    typeof report.javascriptHeap.heapSpaces,
    typeof report.resourceUsage.rss,
    typeof report.resourceUsage.fsActivity,
  ].join(","),
);

errorLine("get bad err", () => process.report.getReport(123 as any));
errorLine("write bad file", () => process.report.writeReport(123 as any));
errorLine("write bad err", () => process.report.writeReport(undefined as any, 123 as any));

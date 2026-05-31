import path from "node:path";

const cwdTail = process.cwd().replace(/\//g, "\\").replace(/^\\+/, "");
const driveCwd = (drive: string) => cwdTail ? `${drive}:\\${cwdTail}` : `${drive}:\\`;
const childOfDriveCwd = (drive: string, child: string) => {
  const base = driveCwd(drive);
  return base.endsWith("\\") ? base + child : `${base}\\${child}`;
};

for (const [label, actual, expected] of [
  ["resolve same drive", path.win32.resolve("C:\\a", "C:foo"), "C:\\a\\foo"],
  ["resolve same drive empty", path.win32.resolve("C:\\a", "C:"), "C:\\a"],
  ["resolve other drive", path.win32.resolve("C:\\a", "D:foo"), childOfDriveCwd("D", "foo")],
  ["resolve rooted then drive", path.win32.resolve("\\root", "C:foo"), "C:\\root\\foo"],
  ["resolve chained drive", path.win32.resolve("C:\\a", "foo", "C:bar"), "C:\\a\\foo\\bar"],
  ["relative drive cwd", path.win32.relative("C:foo", "C:bar"), "..\\bar"],
  ["relative abs to drive", path.win32.relative("C:\\a", "C:bar"), `..\\${cwdTail}\\bar`],
  ["relative other drive", path.win32.relative("C:\\a", "D:bar"), childOfDriveCwd("D", "bar")],
] as const) {
  console.log(label + ":", actual === expected, JSON.stringify(actual));
}

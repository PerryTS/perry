import { App, VStack, Text, Button, Table } from "perry/ui";

const data = [["a"], ["b"]];
const t = Table(data.length, 1, (row: number, col: number) => Text(data[row][col]));

App({
  title: "table-layout-repro-array",
  width: 400,
  height: 200,
  body: VStack(8, [t, Button("noop", () => {})]),
});

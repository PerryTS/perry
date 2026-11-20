import { App, VStack, Text, Button, Table } from "perry/ui";

const t = Table(2, 1, (row: number, _col: number) => Text("row " + row));

App({
  title: "table-layout-repro",
  width: 400,
  height: 200,
  body: VStack(8, [t, Button("noop", () => {})]),
});

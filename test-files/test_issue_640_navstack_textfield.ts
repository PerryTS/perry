import { App, VStack, Text, Button, TextField, NavStack, state } from "perry/ui";

const route = state("a");
let aValue = "";
let bValue = "";

App({
  title: "TextField nav probe",
  width: 400, height: 300,
  body: NavStack(route, [
    {
      name: "a",
      body: VStack(8, [
        Text("route A"),
        TextField("type here on A", (s: string) => { aValue = s; console.log("A:", s); }),
        Button("go to B", () => route.set("b")),
        Button("log A value", () => console.log("aValue=" + aValue)),
      ]),
    },
    {
      name: "b",
      body: VStack(8, [
        Text("route B"),
        TextField("type here on B", (s: string) => { bValue = s; console.log("B:", s); }),
        Button("go to A", () => route.set("a")),
        Button("log B value", () => console.log("bValue=" + bValue)),
      ]),
    },
  ]),
});

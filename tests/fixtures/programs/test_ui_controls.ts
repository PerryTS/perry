import { App, VStack, HStack, Text, Button, State, Spacer, Divider, TextField, Toggle, Slider } from "perry/ui"

const volume = State(50)

App({
  title: "Controls Demo",
  width: 400,
  height: 500,
  body: VStack(16, [
    Text("Settings"),
    Divider(),
    TextField("Enter your name", (text: string) => {
      console.log("Name:", text)
    }),
    Toggle("Enable notifications", (checked: boolean) => {
      console.log("Notifications:", checked)
    }),
    Slider(0, 100, 50, (value: number) => {
      volume.set(value)
    }),
    Spacer(),
    Text("Footer"),
  ])
})

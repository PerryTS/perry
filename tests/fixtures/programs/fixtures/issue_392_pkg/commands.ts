export function processCommands(
  commands: { type: string; componentType: number; component: unknown }[],
  changeset: { set(componentType: number, component: unknown): void },
): void {
  for (const command of commands) {
    if (command.type === "set") {
      changeset.set(command.componentType, command.component);
    }
  }
}

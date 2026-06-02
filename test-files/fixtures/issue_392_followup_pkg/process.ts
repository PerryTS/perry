import type { Changeset } from "./changeset.ts";

export type Command =
  | { type: "set"; componentType: number; component: unknown }
  | { type: "delete"; componentType: number };

function processSetCommand(componentType: number, component: unknown, changeset: Changeset): void {
  changeset.set(componentType, component);
}

export function processCommands(commands: Command[], changeset: Changeset): void {
  for (const command of commands) {
    if (command.type === "set") {
      processSetCommand(command.componentType, command.component, changeset);
    } else if (command.type === "delete") {
      changeset.delete(command.componentType);
    }
  }
}

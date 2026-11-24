# Dialogs

Perry provides native dialog functions for file selection, alerts, and sheets.
Every snippet below is excerpted from
[`docs/examples/ui/dialogs/snippets.ts`](../../examples/ui/dialogs/snippets.ts) —
CI compiles and links the file on every PR, so the API drawn here is the API
the runtime exposes.

All file dialogs are **callback-based** (the OS-modal panel is non-blocking on
Apple platforms, so a synchronous return wouldn't be possible without freezing
the app's run loop). The callback receives an empty string when the user
cancels.

## File Open Dialog

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:open-file}}
```

On the web target the callback receives only the file's name, since a page
cannot read a path. `openFileDialogData(accept, callback)` delivers the name and
the contents as a base64 `data:` URL, ready for an upload request; `accept`
filters the picker (`"image/*"`, `".pdf"`, or `""` for any file). Native
backends open no dialog for it; read the path from `openFileDialog` instead.

```typescript,no-test
openFileDialogData("image/*", (name: string, dataUrl: string) => {
  fetch("/api/photos", { method: "POST", body: JSON.stringify({ name, dataUrl }) });
});
```

## Folder Selection Dialog

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:open-folder}}
```

## Save File Dialog

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:save-file}}
```

`saveFileDialog(callback, defaultName, extension)` pre-fills the name field
with `defaultName.<extension>`.

## Alert

Display a native alert dialog:

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:alert}}
```

`alert(title, message)` shows a modal alert with an OK button.

## Alert with Buttons

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:alert-with-buttons}}
```

`alertWithButtons(title, message, buttons, callback)` invokes the callback
with the 0-based index of the button the user clicked. By convention put a
destructive label last and check the index in the callback.

## Sheets

Sheets are modal panels attached to a window. Build the body, hand it (with a
size) to `sheetCreate`, then `sheetPresent` it. To dismiss programmatically,
keep the handle around and call `sheetDismiss(handle)`:

```typescript
{{#include ../../examples/ui/dialogs/snippets.ts:sheet}}
```

## Platform Notes

| Dialog | macOS | iOS | Windows | Linux | Web |
|--------|-------|-----|---------|-------|-----|
| File Open | NSOpenPanel | UIDocumentPicker | IFileOpenDialog | GtkFileChooserDialog | `<input type="file">` |
| File Save | NSSavePanel | — | IFileSaveDialog | GtkFileChooserDialog | Download link |
| Folder | NSOpenPanel | — | IFileOpenDialog | GtkFileChooserDialog | — |
| Alert | NSAlert | UIAlertController | MessageBoxW | MessageDialog | `alert()` |
| Sheet | NSSheet | Modal VC | Modal Dialog | Modal Window | Modal div |

## Complete Example: minimal text editor

A real program that wires `openFileDialog` and `saveFileDialog` into a
state-bound `TextField`:

```typescript
{{#include ../../examples/ui/dialogs/text_editor.ts}}
```

## Next Steps

- [Menus](menus.md) — Menu bar and context menus
- [Multi-Window](multi-window.md) — Multiple windows
- [Events](events.md) — User interaction events

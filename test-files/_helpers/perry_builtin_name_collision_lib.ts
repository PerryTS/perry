// Helper for test_issue_6087_builtin_name_collision.ts.
//
// Every function here is deliberately named after a row in one of the
// name-keyed perry/* dispatch tables (PERRY_SYSTEM_TABLE,
// PERRY_UPDATER_TABLE, PERRY_BACKGROUND_TABLE). None of them import
// anything native — they are plain user functions that merely happen to
// share a name with a builtin.
//
// Pre-#6087 the codegen hijacked the *call sites* in the importing module
// purely by callee name:
//   - arity != the builtin's row  -> the call was silently dropped
//     (takeScreenshot, getLocale, isDarkMode, schedule below)
//   - arity == the builtin's row  -> the program linked against an
//     undefined `perry_system_*` / `perry_background_*` symbol
//     (hapticPlay, openURL, cancel below)

// PERRY_SYSTEM_TABLE row: `takeScreenshot`, args: [] (arity mismatch)
export function takeScreenshot(path: string): string {
  return "takeScreenshot:" + path;
}

// PERRY_SYSTEM_TABLE row: `hapticPlay`, args: [Str] (arity MATCHES)
export function hapticPlay(style: string): string {
  return "hapticPlay:" + style;
}

// PERRY_SYSTEM_TABLE row: `openURL`, args: [Str] (arity MATCHES)
export function openURL(url: string): string {
  return "openURL:" + url;
}

// PERRY_SYSTEM_TABLE row: `getLocale`, args: [] (arity MATCHES, 0 args)
export function getLocale(): string {
  return "getLocale:user";
}

// PERRY_SYSTEM_TABLE row: `isDarkMode`, args: [] (arity MATCHES, 0 args)
export function isDarkMode(): boolean {
  return true;
}

// PERRY_SYSTEM_TABLE row: `preferencesSet`, args: [Str, Str]
export function preferencesSet(key: string, value: string, extra: number): string {
  return "preferencesSet:" + key + "=" + value + "+" + extra;
}

// PERRY_BACKGROUND_TABLE row: `cancel`
export function cancel(id: string): string {
  return "cancel:" + id;
}

// PERRY_BACKGROUND_TABLE row: `schedule`
export function schedule(id: string, seconds: number, repeats: boolean): string {
  return "schedule:" + id + "/" + seconds + "/" + repeats;
}

// PERRY_UPDATER_TABLE row: `compareVersions`
export function compareVersions(a: string, b: string): number {
  return a === b ? 0 : a < b ? -1 : 1;
}

// PERRY_UPDATER_TABLE row: `relaunch`
export function relaunch(): string {
  return "relaunch:user";
}

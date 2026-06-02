//! No-op FFI stubs for harmonyos builds: `perry_ui_*` (issue #395),
//! `perry_system_*` + `perry_updater_*` (issue #399).
//!
//! HarmonyOS uses the `perry-codegen-arkts` harvest model — the
//! `App({body: VStack([...])})` literal is destructively rewritten into
//! ArkUI source (`Index.ets`). For the harvested widget tree the LLVM
//! codegen never sees `perry_ui_*` calls. There is no
//! `perry-ui-harmonyos` crate by design.
//!
//! BUT — three families of `perry/*` FFI helpers leak into the lowered
//! `.so`:
//!
//! - **perry/ui** (#395): library factory functions like Hone's
//!   `createEditorPerryWidget`, event-handler closure bodies,
//!   conditional widget builders — anything not part of the harvest
//!   target's `App({body: ...})` literal.
//! - **perry/system** (#399): `isDarkMode()`, `getDeviceModel()`,
//!   `keychainSave/get`, `notificationSend`, etc. — never go through
//!   the harvest pass at all.
//! - **perry/updater** (#399): `install`, `verifyHash`,
//!   `compareVersions`, etc. — same shape as perry/system.
//!
//! Without stubs the OHOS dynamic loader rejects the bundle at app
//! launch with `Error relocating ... perry_X: symbol not found` and
//! the program never reaches `main`.
//!
//! These stubs link-resolve every relevant symbol the codegen can
//! emit. They're auto-generated from the `perry-dispatch` four tables
//! (PERRY_UI_TABLE + PERRY_UI_INSTANCE_TABLE + PERRY_SYSTEM_TABLE +
//! PERRY_UPDATER_TABLE — single source of truth, see
//! `perry-runtime/build.rs`), so a new dispatch row automatically gets
//! a stub and there's no whack-a-mole.
//!
//! Each stub returns the zero-value for its declared `ReturnKind`
//! (handle 0 for Widget, 0.0 for F64, no-op for Void, null for Str).
//! The perry/ui call becomes visually a no-op but the app boots and
//! the harvest path's widgets render normally; only non-harvested
//! perry/ui calls degrade. Replace specific stubs with real ArkUI
//! bridges as they're needed by overriding the symbol elsewhere in
//! perry-runtime (the build.rs's `seen` set ensures each symbol is
//! generated at most once, so a manual override in a sibling module
//! would still collide — to override, comment out the dispatch row's
//! generation here AND provide the real impl in arkts_callbacks.rs or
//! similar).

include!(concat!(env!("OUT_DIR"), "/perry_ui_harmonyos_stubs.rs"));

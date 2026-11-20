Bumped `tungstenite` 0.24.0 → 0.30.0 (dependabot) in `perry-ui-android`, the last
consumer still pinned to the older major. The API `crates/perry-ui-android/src/ws.rs`
uses — `connect`, `WebSocket`/`MaybeTlsStream`, and the `Message::Text`/`Close`/
`Ping`/`Pong`/`Frame`/`Binary` variants — is unchanged across 0.24 → 0.30, so no
call sites needed touching; verified with a clean `cargo check -p perry-ui-android`.
This also unifies tungstenite's version in the lockfile: `turnloop-websocket` was
already on 0.30.0, so the tree no longer carries two tungstenite majors side by
side. Updated `scripts/tokio_inventory.json`'s `perry-ui-android`/`tungstenite`
entry (lockfile version list and blocker prose) to match.

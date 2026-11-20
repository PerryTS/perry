`perry-ui-android` compiles again: its WebSocket I/O thread still passed a
`String` to `Message::Text` and queued the received text as a `String`, but
tungstenite 0.30 carries `Utf8Bytes`. Both Android UI release legs failed on
this since the 0.24 -> 0.30 bump.

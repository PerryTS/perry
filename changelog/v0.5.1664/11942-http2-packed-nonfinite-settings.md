**HTTP/2 packed settings retain non-finite numeric values.**

`http2.getPackedSettings()` now matches Node.js when numeric SETTINGS contain
`NaN` or infinity: supported `NaN` values pack as zero, while infinities
produce the expected range error with the original value in its message.

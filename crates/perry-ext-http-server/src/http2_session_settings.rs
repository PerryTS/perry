use perry_ffi::JsValue;

#[derive(Clone)]
pub struct Http2SettingsState {
    pub header_table_size: u32,
    pub enable_push: bool,
    pub initial_window_size: u32,
    pub max_frame_size: u32,
    pub max_concurrent_streams: u32,
    pub max_header_size: u32,
    pub max_header_list_size: u32,
    pub enable_connect_protocol: bool,
}

impl Default for Http2SettingsState {
    fn default() -> Self {
        Self {
            header_table_size: 4096,
            enable_push: true,
            initial_window_size: 65_535,
            max_frame_size: 16_384,
            max_concurrent_streams: u32::MAX,
            max_header_size: 65_535,
            max_header_list_size: 65_535,
            enable_connect_protocol: false,
        }
    }
}

impl Http2SettingsState {
    pub(crate) fn apply_value(&mut self, value: f64) {
        let v = JsValue::from_bits(value.to_bits());
        if v.is_undefined() || v.is_null() || !v.is_pointer() {
            return;
        }
        let Some(json) = perry_ffi::json_stringify(v) else {
            return;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
            return;
        };
        let Some(obj) = parsed.as_object() else {
            return;
        };
        if let Some(v) = obj.get("headerTableSize").and_then(json_u32) {
            self.header_table_size = v;
        }
        if let Some(v) = obj.get("enablePush").and_then(|v| v.as_bool()) {
            self.enable_push = v;
        }
        if let Some(v) = obj.get("initialWindowSize").and_then(json_u32) {
            self.initial_window_size = v;
        }
        if let Some(v) = obj.get("maxFrameSize").and_then(json_u32) {
            self.max_frame_size = v;
        }
        if let Some(v) = obj.get("maxConcurrentStreams").and_then(json_u32) {
            self.max_concurrent_streams = v;
        }
        if let Some(v) = obj.get("maxHeaderSize").and_then(json_u32) {
            self.max_header_size = v;
            self.max_header_list_size = v;
        }
        if let Some(v) = obj.get("maxHeaderListSize").and_then(json_u32) {
            self.max_header_list_size = v;
            self.max_header_size = v;
        }
        if let Some(v) = obj.get("enableConnectProtocol").and_then(|v| v.as_bool()) {
            self.enable_connect_protocol = v;
        }
    }

    pub(crate) fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"headerTableSize\":{},",
                "\"enablePush\":{},",
                "\"initialWindowSize\":{},",
                "\"maxFrameSize\":{},",
                "\"maxConcurrentStreams\":{},",
                "\"maxHeaderSize\":{},",
                "\"maxHeaderListSize\":{},",
                "\"enableConnectProtocol\":{}",
                "}}"
            ),
            self.header_table_size,
            self.enable_push,
            self.initial_window_size,
            self.max_frame_size,
            self.max_concurrent_streams,
            self.max_header_size,
            self.max_header_list_size,
            self.enable_connect_protocol
        )
    }
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    let n = value.as_u64()?;
    u32::try_from(n).ok()
}

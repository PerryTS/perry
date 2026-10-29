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
    /// The SETTINGS identifiers the caller named, as `1 << id`. Node puts only
    /// those on the wire; the fields above still report every value in force.
    pub given: u16,
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
            given: 0,
        }
    }
}

impl Http2SettingsState {
    pub(crate) fn names(&self, id: u16) -> bool {
        self.given & (1 << id) != 0
    }

    /// The (identifier, value) pairs the caller named, in ascending order.
    pub(crate) fn named(&self) -> Vec<(u16, u32)> {
        [
            (1, self.header_table_size),
            (2, self.enable_push as u32),
            (3, self.max_concurrent_streams),
            (4, self.initial_window_size),
            (5, self.max_frame_size),
            (6, self.max_header_list_size),
            (8, self.enable_connect_protocol as u32),
        ]
        .into_iter()
        .filter(|&(id, _)| self.names(id))
        .collect()
    }

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
        self.apply_json(&parsed);
    }

    /// The same merge from an already-parsed object, for option surfaces that
    /// reach Rust as JSON (`createServer({ settings })`) rather than as a
    /// NaN-boxed value.
    pub(crate) fn apply_json(&mut self, parsed: &serde_json::Value) {
        let Some(obj) = parsed.as_object() else {
            return;
        };
        if let Some(v) = obj.get("headerTableSize").and_then(json_u32) {
            self.header_table_size = v;
            self.given |= 1 << 1;
        }
        if let Some(v) = obj.get("enablePush").and_then(|v| v.as_bool()) {
            self.enable_push = v;
            self.given |= 1 << 2;
        }
        if let Some(v) = obj.get("initialWindowSize").and_then(json_u32) {
            self.initial_window_size = v;
            self.given |= 1 << 4;
        }
        if let Some(v) = obj.get("maxFrameSize").and_then(json_u32) {
            self.max_frame_size = v;
            self.given |= 1 << 5;
        }
        if let Some(v) = obj.get("maxConcurrentStreams").and_then(json_u32) {
            self.max_concurrent_streams = v;
            self.given |= 1 << 3;
        }
        if let Some(v) = obj.get("maxHeaderSize").and_then(json_u32) {
            self.max_header_size = v;
            self.max_header_list_size = v;
            self.given |= 1 << 6;
        } else if let Some(v) = obj.get("maxHeaderListSize").and_then(json_u32) {
            self.max_header_list_size = v;
            self.max_header_size = v;
            self.given |= 1 << 6;
        }
        if let Some(v) = obj.get("enableConnectProtocol").and_then(|v| v.as_bool()) {
            self.enable_connect_protocol = v;
            self.given |= 1 << 8;
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

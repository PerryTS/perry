wit_bindgen::generate!({
    inline: r#"
        package perry:transform@1.0.0;
        interface logging { log: func(message: string); }
        interface audit { emit: func(message: string); }
        world logger {
            import audit;
            export logging;
        }
    "#,
    world: "logger",
});

struct Logger;

impl exports::perry::transform::logging::Guest for Logger {
    fn log(message: String) {
        // The Rust host verifies this prefix and the call count, so an
        // unconnected component or a bypassed logger cannot pass silently.
        perry::transform::audit::emit(&format!("Rust logger: {message}"));
    }
}

export!(Logger);

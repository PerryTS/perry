wit_bindgen::generate!({ path: "..", world: "plugin" });

use exports::perry::transform::transform::{Failure, Guest, Request};

struct Transform;

impl Guest for Transform {
    fn transform(mut input: Request) -> Result<Request, Failure> {
        perry::transform::logging::log(&input.text);
        if input.text.is_empty() {
            return Err(Failure::InvalidInput("text must not be empty".into()));
        }
        input.text.push_str(" / Rust");
        input.bytes.reverse();
        Ok(input)
    }
}

export!(Transform);

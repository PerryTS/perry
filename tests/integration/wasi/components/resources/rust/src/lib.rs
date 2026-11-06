wit_bindgen::generate!({ path: "..", world: "plugin" });

use exports::perry::resources::counters::{Counter, Guest, GuestCounter};

struct Counters;
struct CounterValue(u32);

impl GuestCounter for CounterValue {
    fn new(value: u32) -> Self {
        Self(value)
    }
    fn read(&self) -> u32 {
        self.0
    }
}

impl Drop for CounterValue {
    fn drop(&mut self) {
        if self.0 == 13 {
            panic!("deliberate guest destructor trap");
        }
        perry::resources::observe::dropped(self.0);
    }
}

impl Guest for Counters {
    type Counter = CounterValue;

    fn consume(value: Counter) {
        drop(value);
    }
}

export!(Counters);

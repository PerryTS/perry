### Fixed

Root persistent `perry/ui` JavaScript callbacks and state values across garbage
collections on every native UI backend, including callbacks retained by native
timer, hotkey, and focus-event closures.

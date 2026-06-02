// Base side of the caller-drop fixture. `caller` calls `target`, and
// `target` is defined here, so the Calls edge resolves cleanly.

pub fn caller() -> u32 {
    target()
}

pub fn target() -> u32 {
    42
}

// Head side of the caller-drop fixture. `target` was removed but `caller`
// still calls it — the Calls edge is left unresolved, which is exactly the
// caller-drop breakage the review must surface. `caller` is byte-identical
// to the base side so it does not register as a modified-body change.

pub fn caller() -> u32 {
    target()
}

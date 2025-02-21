#[derive(Copy, Clone, PartialEq)]
/// A key
pub enum Key {
    Char(char),
    Alt(bool),
    Shift(bool),
    Ctrl(bool),
    // TODO: Space modifier?
    Backspace,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Tab,
    Unknown(u8),
}

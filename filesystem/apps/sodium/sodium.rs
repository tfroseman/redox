pub use redox::collections::VecDeque;


mod editor;
pub use self::editor::*;

mod parse;
pub use self::parse::*;

mod keystate;
pub use self::keystate::*;

mod key;
pub use self::key::*;

mod position;
pub use self::position::*;

mod graphics;
pub use self::graphics::*;

mod selection;
pub use self::selection::*;

mod mode;
pub use self::mode::*;

mod movement;
pub use self::movement::*;

mod motion;
pub use self::motion::*;

mod cursor;
pub use self::cursor::*;

mod insert;
pub use self::insert::*;

mod delete;
pub use self::delete::*;

mod exec;
pub use self::exec::*;

pub fn main() {
    let editor = Editor::new();
}

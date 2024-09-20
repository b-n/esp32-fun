#[derive(Debug, Copy, Clone)]
pub enum Event {
    On(i32),
    Off(i32),
    Pressed(i32),
    Released(i32),
}

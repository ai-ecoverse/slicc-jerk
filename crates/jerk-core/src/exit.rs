#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Ok,
    Error,
    Usage,
    SignIn,
    Unavailable,
    Unsupported,
}

impl From<Exit> for i32 {
    fn from(exit: Exit) -> i32 {
        match exit {
            Exit::Ok => 0,
            Exit::Error => 1,
            Exit::Usage => 2,
            Exit::SignIn => 3,
            Exit::Unavailable => 4,
            Exit::Unsupported => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Placeholder,
    Token,
    Masked,
}

pub fn choose(placeholder: bool, terminal: bool, reveal: bool) -> Shown {
    if placeholder {
        Shown::Placeholder
    } else if terminal || reveal {
        Shown::Token
    } else {
        Shown::Masked
    }
}

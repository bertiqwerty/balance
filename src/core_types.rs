use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct BlcError {
    pub msg: String,
}
impl BlcError {
    pub fn new(msg: &str) -> Self {
        BlcError {
            msg: msg.to_string(),
        }
    }
}
#[macro_export]
macro_rules! blcerr {
    ($s:literal $(, $exps:expr )*) => {
        $crate::core_types::BlcError::new(format!($s, $($exps,)*).as_str())
    }
}

pub type BlcResult<T> = Result<T, BlcError>;

pub fn to_blc<E: Debug>(e: E) -> BlcError {
    BlcError {
        msg: format!("{e:?}"),
    }
}

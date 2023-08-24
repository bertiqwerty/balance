use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::{Debug, Display},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
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
impl Display for BlcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}
impl Error for BlcError {}
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

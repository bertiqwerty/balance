use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct BalError {
    pub msg: String,
}

pub type BalResult<T> = Result<T, BalError>;

pub fn to_bres<E: Debug>(e: E) -> BalError {
    BalError {
        msg: format!("{:?}", e),
    }
}

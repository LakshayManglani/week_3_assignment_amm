use anchor_lang::prelude::*;

#[error_code]
pub enum AmmError {
    #[msg("Deposit amount must be greater than zero")]
    ZeroAmount,

    #[msg("Output amount is less than minimum expected (slippage exceeded)")]
    SlippageExceeded,

    #[msg("Pool is currently locked")]
    PoolLocked,

    #[msg("Curve calculation error")]
    CurveError,
}

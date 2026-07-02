use anchor_lang::prelude::*;

#[error_code]
pub enum LaunchError {
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Requested growth rate exceeds the cap for this tier")]
    RateExceedsTierCap,
    #[msg("Invalid tier (must be 0..=3)")]
    InvalidTier,
    #[msg("Bonding curve already graduated; trade on the AMM")]
    CurveComplete,
    #[msg("Bonding curve has not graduated yet")]
    CurveNotComplete,
    #[msg("Slippage: output below minimum requested")]
    SlippageExceeded,
    #[msg("Not enough tokens left on the curve")]
    InsufficientCurveTokens,
    #[msg("Not enough SOL in the curve")]
    InsufficientCurveSol,
    #[msg("Crank called too soon (min interval not elapsed)")]
    CrankTooSoon,
    #[msg("Nothing to accrue")]
    NothingToAccrue,
    #[msg("New rate may only be lowered, and must stay under the tier cap")]
    InvalidRateUpdate,
    #[msg("Unauthorized")]
    Unauthorized,
}

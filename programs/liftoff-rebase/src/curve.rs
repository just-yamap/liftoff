//! Constant-product bonding curve with virtual reserves (pump.fun model).
//! ALL math here operates on RAW token amounts. The display multiplier from
//! the Scaled UI Amount extension must NEVER enter pricing.

use anchor_lang::prelude::*;

use crate::errors::LaunchError;

/// Raw tokens received for `sol_in` lamports (after fee has been deducted).
pub fn tokens_out_for_sol_in(
    virtual_sol: u64,
    virtual_token: u64,
    sol_in: u64,
) -> Result<u64> {
    // tokens_out = vTOKEN - k / (vSOL + sol_in)
    let k = (virtual_sol as u128)
        .checked_mul(virtual_token as u128)
        .ok_or(LaunchError::MathOverflow)?;
    let new_sol = (virtual_sol as u128)
        .checked_add(sol_in as u128)
        .ok_or(LaunchError::MathOverflow)?;
    let new_token = k
        .checked_div(new_sol)
        .ok_or(LaunchError::MathOverflow)?
        .checked_add(1) // round against the buyer
        .ok_or(LaunchError::MathOverflow)?;
    let out = (virtual_token as u128)
        .checked_sub(new_token)
        .ok_or(LaunchError::MathOverflow)?;
    u64::try_from(out).map_err(|_| error!(LaunchError::MathOverflow))
}

/// Lamports received for `tokens_in` raw tokens (before fee).
pub fn sol_out_for_tokens_in(
    virtual_sol: u64,
    virtual_token: u64,
    tokens_in: u64,
) -> Result<u64> {
    // sol_out = vSOL - k / (vTOKEN + tokens_in)
    let k = (virtual_sol as u128)
        .checked_mul(virtual_token as u128)
        .ok_or(LaunchError::MathOverflow)?;
    let new_token = (virtual_token as u128)
        .checked_add(tokens_in as u128)
        .ok_or(LaunchError::MathOverflow)?;
    let new_sol = k
        .checked_div(new_token)
        .ok_or(LaunchError::MathOverflow)?
        .checked_add(1) // round against the seller
        .ok_or(LaunchError::MathOverflow)?;
    let out = (virtual_sol as u128)
        .checked_sub(new_sol)
        .ok_or(LaunchError::MathOverflow)?;
    u64::try_from(out).map_err(|_| error!(LaunchError::MathOverflow))
}

pub fn fee(amount: u64, fee_bps: u16) -> Result<u64> {
    let f = (amount as u128)
        .checked_mul(fee_bps as u128)
        .ok_or(LaunchError::MathOverflow)?
        / 10_000u128;
    u64::try_from(f).map_err(|_| error!(LaunchError::MathOverflow))
}

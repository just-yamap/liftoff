use anchor_lang::prelude::*;

#[account]
pub struct Config {
    pub admin: Pubkey,
    pub attestor: Pubkey,
    pub fee_recipient: Pubkey,
    pub fee_bps: u16,
    pub graduation_lamports: u64,
    pub bump: u8,
}

impl Config {
    pub const LEN: usize = 8 + 32 * 3 + 2 + 8 + 1;
}

#[account]
pub struct BondingCurve {
    pub creator: Pubkey,
    pub mint: Pubkey,
    /// Monster Club tier attested at creation (0..=3).
    pub tier: u8,
    /// Growth rate in basis points per year (display multiplier growth).
    pub rate_bps: u32,
    /// Current display multiplier (mirrors the on-chain extension value).
    pub multiplier: f64,
    pub last_crank_ts: i64,
    pub created_at: i64,
    /// Virtual reserves — RAW amounts only. Multiplier never enters pricing.
    pub virtual_sol: u64,
    pub virtual_token: u64,
    /// Real reserves backing trades.
    pub real_sol: u64,
    pub real_token: u64,
    pub complete: bool,
    pub bump: u8,
}

impl BondingCurve {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 4 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 1;
}

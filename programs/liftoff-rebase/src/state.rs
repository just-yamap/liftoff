use anchor_lang::prelude::*;

#[account]
pub struct Config {
    pub admin: Pubkey,
    pub attestor: Pubkey,
    pub fee_recipient: Pubkey,
    pub fee_bps: u16,
    /// Creator fee on pre-migration trades (docs: 0.05% = 5 bps).
    pub creator_fee_bps: u16,
    /// Default graduation threshold (per-token value overrides).
    pub graduation_lamports: u64,
    pub bump: u8,
}

impl Config {
    pub const LEN: usize = 8 + 32 * 3 + 2 + 2 + 8 + 1;
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
    /// Per-token graduation threshold (30-300 SOL, docs: custom bonding).
    pub graduation_lamports: u64,
    /// Timed launch: trading disabled before this unix timestamp.
    pub trade_open_ts: i64,
    /// Bonding burn action: bps of leftover curve tokens burned at migration
    /// (Normal=0, Mega=7500, Ultra=9000, Degen=9900).
    pub burn_bps_on_migrate: u16,
    pub bump: u8,
}

impl BondingCurve {
    pub const LEN: usize = 8 + 32 + 32 + 1 + 4 + 8 + 8 + 8 + 8 + 8 + 8 + 8 + 1 + 8 + 8 + 2 + 1;
}

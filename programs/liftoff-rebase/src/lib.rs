//! Liftoff Rebase — pump.fun-style launchpad with display-rebasing tokens.
//!
//! Mechanic: Token-2022 Scaled UI Amount extension. Holders' *displayed*
//! balances grow as `raw_balance × multiplier`; a permissionless `crank`
//! advances the multiplier by `rate × elapsed / year` per interval (which
//! compounds naturally across cranks). Raw balances and all curve math never
//! change — this is supply-display growth, not yield.
//!
//! Safety invariants:
//!   * Multiplier authority = per-token curve PDA. Creators can never touch it.
//!   * Curve math uses RAW amounts only.
//!   * Tier caps enforced on-chain; tier attested by the backend co-signer.
//!   * Per-interval linear accrual instead of f64::powf (unreliable in SBF).

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::{invoke, invoke_signed};
use anchor_lang::system_program;
use anchor_spl::associated_token::{self, AssociatedToken};

use anchor_spl::token_interface::{
    self, Mint as MintInterface, Token2022, TokenAccount as TokenAccountInterface,
    TransferChecked,
};
use spl_token_2022::extension::ExtensionType;
use spl_token_2022::state::Mint as MintState;
use spl_token_metadata_interface::state::TokenMetadata;

pub mod curve;
pub mod errors;
pub mod state;

use curve::*;
use errors::LaunchError;
use state::*;

declare_id!("AoVUouTT7TqwruCcseNe6BSETKkDV5mvcjaUbN83B8h6");

/// 1B tokens, 6 decimals (pump.fun convention).
pub const TOKEN_DECIMALS: u8 = 6;
pub const TOTAL_SUPPLY: u64 = 1_000_000_000 * 1_000_000;
/// Initial virtual reserves (pump.fun model).
pub const VIRTUAL_SOL_0: u64 = 30_000_000_000; // 30 SOL
pub const VIRTUAL_TOKEN_0: u64 = 1_073_000_000 * 1_000_000; // 1.073B raw
/// Tokens actually sellable on the curve; remainder migrates to the AMM.
pub const CURVE_TOKENS: u64 = 793_100_000 * 1_000_000;

/// Max growth rate (bps/year) per Monster Club tier. Tier 3 is the hard
/// safety cap: 1,000,000% keeps the multiplier well-formed for years of cranks.
pub const MAX_RATE_BPS_BY_TIER: [u32; 4] = [3_300, 100_000, 1_000_000, 100_000_000];

pub const MIN_CRANK_INTERVAL: i64 = 300; // 5 minutes
pub const SECONDS_PER_YEAR: f64 = 31_536_000.0;
/// Hard ceiling so extreme rates can never produce a broken f64.
pub const MAX_MULTIPLIER: f64 = 1e12;

pub const CONFIG_SEED: &[u8] = b"config";
pub const CURVE_SEED: &[u8] = b"curve";

#[program]
pub mod liftoff_rebase {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        attestor: Pubkey,
        fee_recipient: Pubkey,
        fee_bps: u16,
        graduation_lamports: u64,
    ) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.attestor = attestor;
        config.fee_recipient = fee_recipient;
        config.fee_bps = fee_bps;
        config.graduation_lamports = graduation_lamports;
        config.bump = ctx.bumps.config;
        Ok(())
    }

    /// Create a rebasing token: Token-2022 mint with Scaled UI Amount
    /// (multiplier authority = curve PDA) + on-chain metadata, mint the full
    /// supply to the curve, then revoke mint authority. No freeze authority.
    /// Requires the platform attestor co-signature (tier verification).
    pub fn create_token(
        ctx: Context<CreateToken>,
        name: String,
        symbol: String,
        uri: String,
        tier: u8,
        rate_bps: u32,
    ) -> Result<()> {
        require!(tier <= 3, LaunchError::InvalidTier);
        require!(
            rate_bps <= MAX_RATE_BPS_BY_TIER[tier as usize],
            LaunchError::RateExceedsTierCap
        );

        let mint_key = ctx.accounts.mint.key();
        let curve_key = ctx.accounts.curve.key();
        let token_program_id = ctx.accounts.token_program.key();
        let curve_bump = ctx.bumps.curve;
        let curve_seeds: &[&[u8]] = &[CURVE_SEED, mint_key.as_ref(), &[curve_bump]];

        // ---- 1. Create the mint account with space for extensions ----
        let space = ExtensionType::try_calculate_account_len::<MintState>(&[
            ExtensionType::ScaledUiAmount,
            ExtensionType::MetadataPointer,
        ])
        .map_err(|_| error!(LaunchError::MathOverflow))?;
        let rent = Rent::get()?;
        let lamports = rent.minimum_balance(space);

        system_program::create_account(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::CreateAccount {
                    from: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.mint.to_account_info(),
                },
            ),
            lamports,
            space as u64,
            &token_program_id,
        )?;

        // ---- 2. Init extensions (must precede initialize_mint2) ----
        // Scaled UI Amount: multiplier 1.0, authority = curve PDA (never creator).
        let ix = spl_token_2022::extension::scaled_ui_amount::instruction::initialize(
            &token_program_id,
            &mint_key,
            Some(curve_key),
            1.0,
        )?;
        invoke(&ix, &[ctx.accounts.mint.to_account_info()])?;

        // Metadata pointer: metadata lives in the mint itself.
        let ix = spl_token_2022::extension::metadata_pointer::instruction::initialize(
            &token_program_id,
            &mint_key,
            Some(curve_key),
            Some(mint_key),
        )?;
        invoke(&ix, &[ctx.accounts.mint.to_account_info()])?;

        // ---- 3. Initialize the mint: authority = curve PDA, no freeze ----
        let ix = spl_token_2022::instruction::initialize_mint2(
            &token_program_id,
            &mint_key,
            &curve_key,
            None,
            TOKEN_DECIMALS,
        )?;
        invoke(&ix, &[ctx.accounts.mint.to_account_info()])?;

        // ---- 4. Write token metadata into the mint (fund extra rent first) ----
        let metadata = TokenMetadata {
            update_authority: Some(curve_key).try_into().unwrap(),
            mint: mint_key,
            name: name.clone(),
            symbol: symbol.clone(),
            uri: uri.clone(),
            additional_metadata: vec![],
        };
        let extra = metadata
            .tlv_size_of()
            .map_err(|_| error!(LaunchError::MathOverflow))?;
        let needed = rent.minimum_balance(space + extra);
        let current = ctx.accounts.mint.to_account_info().lamports();
        if needed > current {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.creator.to_account_info(),
                        to: ctx.accounts.mint.to_account_info(),
                    },
                ),
                needed - current,
            )?;
        }
        let ix = spl_token_metadata_interface::instruction::initialize(
            &token_program_id,
            &mint_key, // metadata account = mint
            &curve_key, // update authority
            &mint_key,
            &curve_key, // mint authority (curve PDA signs)
            name,
            symbol,
            uri,
        );
        invoke_signed(
            &ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.curve.to_account_info(),
            ],
            &[curve_seeds],
        )?;

        // ---- 5. Create the curve's ATA and mint the full supply into it ----
        associated_token::create(CpiContext::new(
            ctx.accounts.associated_token_program.to_account_info(),
            associated_token::Create {
                payer: ctx.accounts.creator.to_account_info(),
                associated_token: ctx.accounts.curve_ata.to_account_info(),
                authority: ctx.accounts.curve.to_account_info(),
                mint: ctx.accounts.mint.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
        ))?;

        let ix = spl_token_2022::instruction::mint_to(
            &token_program_id,
            &mint_key,
            &ctx.accounts.curve_ata.key(),
            &curve_key,
            &[],
            TOTAL_SUPPLY,
        )?;
        invoke_signed(
            &ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.curve_ata.to_account_info(),
                ctx.accounts.curve.to_account_info(),
            ],
            &[curve_seeds],
        )?;

        // ---- 6. Revoke mint authority forever ----
        let ix = spl_token_2022::instruction::set_authority(
            &token_program_id,
            &mint_key,
            None,
            spl_token_2022::instruction::AuthorityType::MintTokens,
            &curve_key,
            &[],
        )?;
        invoke_signed(
            &ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.curve.to_account_info(),
            ],
            &[curve_seeds],
        )?;

        // ---- 7. Init curve state ----
        let now = Clock::get()?.unix_timestamp;
        let bc = &mut ctx.accounts.curve;
        bc.creator = ctx.accounts.creator.key();
        bc.mint = mint_key;
        bc.tier = tier;
        bc.rate_bps = rate_bps;
        bc.multiplier = 1.0;
        bc.last_crank_ts = now;
        bc.created_at = now;
        bc.virtual_sol = VIRTUAL_SOL_0;
        bc.virtual_token = VIRTUAL_TOKEN_0;
        bc.real_sol = 0;
        bc.real_token = CURVE_TOKENS;
        bc.complete = false;
        bc.bump = curve_bump;

        Ok(())
    }

    /// Buy raw tokens with SOL along the bonding curve. 
    pub fn buy(ctx: Context<Trade>, sol_in: u64, min_tokens_out: u64) -> Result<()> {
        let bc = &mut ctx.accounts.curve;
        require!(!bc.complete, LaunchError::CurveComplete);

        let fee_amt = fee(sol_in, ctx.accounts.config.fee_bps)?;
        let net = sol_in
            .checked_sub(fee_amt)
            .ok_or(LaunchError::MathOverflow)?;

        let tokens_out = tokens_out_for_sol_in(bc.virtual_sol, bc.virtual_token, net)?;
        require!(tokens_out >= min_tokens_out, LaunchError::SlippageExceeded);
        require!(
            tokens_out <= bc.real_token,
            LaunchError::InsufficientCurveTokens
        );

        // SOL: trader -> curve PDA (net) and trader -> fee recipient (fee).
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.trader.to_account_info(),
                    to: ctx.accounts.curve.to_account_info(),
                },
            ),
            net,
        )?;
        if fee_amt > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.trader.to_account_info(),
                        to: ctx.accounts.fee_recipient.to_account_info(),
                    },
                ),
                fee_amt,
            )?;
        }

        // Tokens: curve ATA -> trader ATA, curve PDA signs.
        let bc = &mut ctx.accounts.curve;
        let mint_key = bc.mint;
        let curve_seeds: &[&[u8]] = &[CURVE_SEED, mint_key.as_ref(), &[bc.bump]];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.curve_ata.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.trader_ata.to_account_info(),
                    authority: ctx.accounts.curve.to_account_info(),
                },
                &[curve_seeds],
            ),
            tokens_out,
            TOKEN_DECIMALS,
        )?;

        let bc = &mut ctx.accounts.curve;
        bc.virtual_sol = bc
            .virtual_sol
            .checked_add(net)
            .ok_or(LaunchError::MathOverflow)?;
        bc.virtual_token = bc
            .virtual_token
            .checked_sub(tokens_out)
            .ok_or(LaunchError::MathOverflow)?;
        bc.real_sol = bc
            .real_sol
            .checked_add(net)
            .ok_or(LaunchError::MathOverflow)?;
        bc.real_token = bc
            .real_token
            .checked_sub(tokens_out)
            .ok_or(LaunchError::MathOverflow)?;

        if bc.real_sol >= ctx.accounts.config.graduation_lamports {
            bc.complete = true;
        }
        Ok(())
    }

    /// Sell raw tokens for SOL along the bonding curve.
    pub fn sell(ctx: Context<Trade>, tokens_in: u64, min_sol_out: u64) -> Result<()> {
        let bc = &ctx.accounts.curve;
        require!(!bc.complete, LaunchError::CurveComplete);

        let gross = sol_out_for_tokens_in(bc.virtual_sol, bc.virtual_token, tokens_in)?;
        require!(gross <= bc.real_sol, LaunchError::InsufficientCurveSol);
        let fee_amt = fee(gross, ctx.accounts.config.fee_bps)?;
        let net = gross
            .checked_sub(fee_amt)
            .ok_or(LaunchError::MathOverflow)?;
        require!(net >= min_sol_out, LaunchError::SlippageExceeded);

        // Tokens: trader ATA -> curve ATA (trader signs).
        token_interface::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.trader_ata.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.curve_ata.to_account_info(),
                    authority: ctx.accounts.trader.to_account_info(),
                },
            ),
            tokens_in,
            TOKEN_DECIMALS,
        )?;

        // SOL: curve PDA -> trader (+ fee recipient). Curve carries data,
        // so move lamports directly; keep the PDA rent-exempt.
        let curve_info = ctx.accounts.curve.to_account_info();
        let rent_min = Rent::get()?.minimum_balance(curve_info.data_len());
        let after = curve_info
            .lamports()
            .checked_sub(gross)
            .ok_or(LaunchError::InsufficientCurveSol)?;
        require!(after >= rent_min, LaunchError::InsufficientCurveSol);

        **curve_info.try_borrow_mut_lamports()? -= gross;
        **ctx.accounts.trader.to_account_info().try_borrow_mut_lamports()? += net;
        **ctx
            .accounts
            .fee_recipient
            .to_account_info()
            .try_borrow_mut_lamports()? += fee_amt;

        let bc = &mut ctx.accounts.curve;
        bc.virtual_sol = bc
            .virtual_sol
            .checked_sub(gross)
            .ok_or(LaunchError::MathOverflow)?;
        bc.virtual_token = bc
            .virtual_token
            .checked_add(tokens_in)
            .ok_or(LaunchError::MathOverflow)?;
        bc.real_sol = bc
            .real_sol
            .checked_sub(gross)
            .ok_or(LaunchError::MathOverflow)?;
        bc.real_token = bc
            .real_token
            .checked_add(tokens_in)
            .ok_or(LaunchError::MathOverflow)?;
        Ok(())
    }

    /// Permissionless rebase crank. Advances the display multiplier by
    /// linear per-interval accrual: m' = m * (1 + rate * dt / year).
    /// Elapsed time is computed on-chain — callers cannot manipulate it.
    pub fn crank(ctx: Context<Crank>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let bc = &mut ctx.accounts.curve;

        let dt = now
            .checked_sub(bc.last_crank_ts)
            .ok_or(LaunchError::MathOverflow)?;
        require!(dt >= MIN_CRANK_INTERVAL, LaunchError::CrankTooSoon);

        let rate = bc.rate_bps as f64 / 10_000.0;
        let growth = rate * (dt as f64) / SECONDS_PER_YEAR;
        require!(growth > 0.0, LaunchError::NothingToAccrue);

        let mut new_mult = bc.multiplier * (1.0 + growth);
        if new_mult > MAX_MULTIPLIER {
            new_mult = MAX_MULTIPLIER;
        }
        require!(
            new_mult.is_finite() && new_mult >= bc.multiplier,
            LaunchError::MathOverflow
        );

        let mint_key = bc.mint;
        let curve_seeds: &[&[u8]] = &[CURVE_SEED, mint_key.as_ref(), &[bc.bump]];
        let ix = spl_token_2022::extension::scaled_ui_amount::instruction::update_multiplier(
            &ctx.accounts.token_program.key(),
            &mint_key,
            &ctx.accounts.curve.key(),
            &[],
            new_mult,
            now,
        )?;
        invoke_signed(
            &ix,
            &[
                ctx.accounts.mint.to_account_info(),
                ctx.accounts.curve.to_account_info(),
            ],
            &[curve_seeds],
        )?;

        let bc = &mut ctx.accounts.curve;
        bc.multiplier = new_mult;
        bc.last_crank_ts = now;
        Ok(())
    }

    /// Creators can only LOWER their growth rate, never raise it.
    pub fn update_rate(ctx: Context<UpdateRate>, new_rate_bps: u32) -> Result<()> {
        let bc = &mut ctx.accounts.curve;
        require!(
            new_rate_bps < bc.rate_bps,
            LaunchError::InvalidRateUpdate
        );
        bc.rate_bps = new_rate_bps;
        Ok(())
    }

    /// Admin sweep after graduation for AMM pool seeding.
    /// MVP pattern (same as pump-style clones) — replace with a direct AMM
    /// CPI for trustless graduation before scale.
    pub fn migrate(ctx: Context<Migrate>) -> Result<()> {
        let bc = &ctx.accounts.curve;
        require!(bc.complete, LaunchError::CurveNotComplete);

        // Sweep remaining tokens to the admin's ATA.
        let mint_key = bc.mint;
        let curve_seeds: &[&[u8]] = &[CURVE_SEED, mint_key.as_ref(), &[bc.bump]];
        let remaining = ctx.accounts.curve_ata.amount;
        if remaining > 0 {
            token_interface::transfer_checked(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TransferChecked {
                        from: ctx.accounts.curve_ata.to_account_info(),
                        mint: ctx.accounts.mint.to_account_info(),
                        to: ctx.accounts.admin_ata.to_account_info(),
                        authority: ctx.accounts.curve.to_account_info(),
                    },
                    &[curve_seeds],
                ),
                remaining,
                TOKEN_DECIMALS,
            )?;
        }

        // Sweep SOL above rent minimum.
        let curve_info = ctx.accounts.curve.to_account_info();
        let rent_min = Rent::get()?.minimum_balance(curve_info.data_len());
        let sweep = curve_info.lamports().saturating_sub(rent_min);
        if sweep > 0 {
            **curve_info.try_borrow_mut_lamports()? -= sweep;
            **ctx.accounts.admin.to_account_info().try_borrow_mut_lamports()? += sweep;
        }

        let bc = &mut ctx.accounts.curve;
        bc.real_sol = 0;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = Config::LEN,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreateToken<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    /// Platform co-signer attesting the creator's Monster Club tier.
    #[account(constraint = attestor.key() == config.attestor @ LaunchError::Unauthorized)]
    pub attestor: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    /// New mint, created + initialized inside the handler (needs extension
    /// space calculated at runtime).
    #[account(mut)]
    pub mint: Signer<'info>,
    #[account(
        init,
        payer = creator,
        space = BondingCurve::LEN,
        seeds = [CURVE_SEED, mint.key().as_ref()],
        bump
    )]
    pub curve: Account<'info, BondingCurve>,
    /// CHECK: created in-handler as the curve PDA's ATA via CPI to the
    /// associated token program, which validates the derivation.
    #[account(mut)]
    pub curve_ata: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Trade<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(address = curve.mint)]
    pub mint: InterfaceAccount<'info, MintInterface>,
    #[account(
        mut,
        seeds = [CURVE_SEED, mint.key().as_ref()],
        bump = curve.bump
    )]
    pub curve: Account<'info, BondingCurve>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = curve,
        associated_token::token_program = token_program
    )]
    pub curve_ata: InterfaceAccount<'info, TokenAccountInterface>,
    #[account(
        init_if_needed,
        payer = trader,
        associated_token::mint = mint,
        associated_token::authority = trader,
        associated_token::token_program = token_program
    )]
    pub trader_ata: InterfaceAccount<'info, TokenAccountInterface>,
    /// CHECK: validated against config.
    #[account(mut, address = config.fee_recipient @ LaunchError::Unauthorized)]
    pub fee_recipient: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Crank<'info> {
    /// Anyone may crank; typically the keeper.
    pub payer: Signer<'info>,
    #[account(mut, address = curve.mint)]
    pub mint: InterfaceAccount<'info, MintInterface>,
    #[account(
        mut,
        seeds = [CURVE_SEED, mint.key().as_ref()],
        bump = curve.bump
    )]
    pub curve: Account<'info, BondingCurve>,
    pub token_program: Program<'info, Token2022>,
}

#[derive(Accounts)]
pub struct UpdateRate<'info> {
    #[account(constraint = creator.key() == curve.creator @ LaunchError::Unauthorized)]
    pub creator: Signer<'info>,
    #[account(mut)]
    pub curve: Account<'info, BondingCurve>,
}

#[derive(Accounts)]
pub struct Migrate<'info> {
    #[account(mut, constraint = admin.key() == config.admin @ LaunchError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(address = curve.mint)]
    pub mint: InterfaceAccount<'info, MintInterface>,
    #[account(
        mut,
        seeds = [CURVE_SEED, mint.key().as_ref()],
        bump = curve.bump
    )]
    pub curve: Account<'info, BondingCurve>,
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = curve,
        associated_token::token_program = token_program
    )]
    pub curve_ata: InterfaceAccount<'info, TokenAccountInterface>,
    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = mint,
        associated_token::authority = admin,
        associated_token::token_program = token_program
    )]
    pub admin_ata: InterfaceAccount<'info, TokenAccountInterface>,
    pub token_program: Program<'info, Token2022>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

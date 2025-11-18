use anchor_lang::{
    prelude::*,
    solana_program::{program::invoke_signed, system_instruction},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer},
};

declare_id!("EcYQSDeJyV4VpFUbBQbVFzdd1Ta4ZFvQY7ViCUGWd1EY");

const DAY_IN_SECONDS: u64 = 86_400;
const MAX_BPS: u16 = 10_000;
const REFERRAL_BONUS_BPS: u16 = 500; // 5%

#[program]
pub mod vault {
    use super::*;

    pub fn initialize_vault(ctx: Context<InitializeVault>, base_points_rate: u64) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        vault.authority = ctx.accounts.authority.key();
        let bumps = ctx.bumps;
        vault.bump = bumps.vault;
        vault.signer_bump = bumps.vault_signer;
        vault.base_points_rate = base_points_rate;
        vault.paused = false;
        Ok(())
    }

    pub fn update_vault_params(
        ctx: Context<UpdateVault>,
        new_base_rate: u64,
        paused: bool,
    ) -> Result<()> {
        let vault = &mut ctx.accounts.vault;
        require_keys_eq!(vault.authority, ctx.accounts.authority.key(), VaultError::Unauthorized);
        vault.base_points_rate = new_base_rate;
        vault.paused = paused;
        Ok(())
    }

    pub fn register_token(
        ctx: Context<RegisterToken>,
        points_multiplier_bps: u16,
    ) -> Result<()> {
        require!(points_multiplier_bps > 0, VaultError::InvalidMultiplier);
        require!(points_multiplier_bps <= MAX_BPS, VaultError::InvalidMultiplier);

        let vault_token = &mut ctx.accounts.vault_token;
        vault_token.vault = ctx.accounts.vault.key();
        vault_token.mint = ctx.accounts.mint.key();
        vault_token.vault_token_account = ctx.accounts.vault_token_account.key();
        vault_token.points_multiplier_bps = points_multiplier_bps;
        vault_token.bump = ctx.bumps.vault_token;
        vault_token.total_shares = 0;
        vault_token.total_underlying = 0;

        emit!(TokenRegistered {
            mint: ctx.accounts.mint.key(),
            multiplier_bps: points_multiplier_bps
        });
        Ok(())
    }

    pub fn register_strategy(
        ctx: Context<RegisterStrategy>,
        strategy_id: u8,
        weight_bps: u16,
    ) -> Result<()> {
        require!(weight_bps <= MAX_BPS, VaultError::InvalidMultiplier);

        let strategy = &mut ctx.accounts.strategy;
        strategy.vault_token = ctx.accounts.vault_token.key();
        strategy.strategy_id = strategy_id;
        strategy.weight_bps = weight_bps;
        strategy.authority = ctx.accounts.authority.key();
        strategy.last_harvest_ts = 0;

        emit!(StrategyRegistered {
            vault_token: ctx.accounts.vault_token.key(),
            strategy: ctx.accounts.strategy.key(),
            weight_bps,
        });
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64, inviter: Pubkey) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);
        let vault = &ctx.accounts.vault;
        require!(!vault.paused, VaultError::VaultPaused);

        let clock = Clock::get()?;
        let vault_token = &mut ctx.accounts.vault_token;
        let user_position = &mut ctx.accounts.user_position;

        // Validate inviter
        let referral_info = ctx.accounts.referral_record.to_account_info();
        let user_info = ctx.accounts.user.to_account_info();
        let system_program_info = ctx.accounts.system_program.to_account_info();
        let (user_referral_key, user_referral_bump) =
            ReferralRecord::pda(&vault.key(), &ctx.accounts.user.key(), ctx.program_id);
        require_keys_eq!(
            referral_info.key(),
            user_referral_key,
            VaultError::InvalidReferralAccount
        );
        ensure_referral_account(
            &referral_info,
            &user_info,
            &system_program_info,
            &[
                ReferralRecord::SEED,
                vault.key().as_ref(),
                ctx.accounts.user.key().as_ref(),
                &[user_referral_bump],
            ],
            ctx.program_id,
        )?;
        let mut referral_record = read_referral(&referral_info)?;

        if inviter != Pubkey::default() {
            require_keys_neq!(inviter, ctx.accounts.user.key(), VaultError::InvalidInviter);
            if referral_record.inviter == Pubkey::default() {
                referral_record.inviter = inviter;
            } else {
                require_keys_eq!(referral_record.inviter, inviter, VaultError::InviterLocked);
            }
        } else {
            require!(
                referral_record.inviter == Pubkey::default(),
                VaultError::InviterLocked
            );
        }

        // Transfer tokens into vault
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        let shares = calculate_shares(amount, vault_token)?;
        require!(shares > 0, VaultError::ZeroShares);

        accrue_points(
            vault,
            vault_token,
            user_position,
            clock.unix_timestamp,
            AccrualSource::Deposit,
        )?;

        // bootstrap position fields if first time
        if user_position.user == Pubkey::default() {
            user_position.user = ctx.accounts.user.key();
            user_position.vault_token = vault_token.key();
            user_position.bump = ctx.bumps.user_position;
            user_position.last_points_ts = clock.unix_timestamp;
        }

        user_position.shares = user_position
            .shares
            .checked_add(shares)
            .ok_or(VaultError::MathOverflow)?;

        vault_token.total_shares = vault_token
            .total_shares
            .checked_add(shares)
            .ok_or(VaultError::MathOverflow)?;
        vault_token.total_underlying = vault_token
            .total_underlying
            .checked_add(amount)
            .ok_or(VaultError::MathOverflow)?;

        // Referral accounting
        if referral_record.user == Pubkey::default() {
            referral_record.user = ctx.accounts.user.key();
            referral_record.vault = vault.key();
        }

        if referral_record.inviter != Pubkey::default() {
            let (expected_key, inviter_bump) =
                ReferralRecord::pda(&vault.key(), &referral_record.inviter, ctx.program_id);
            let inviter_account = ctx
                .accounts
                .inviter_record
                .as_ref()
                .ok_or(VaultError::InviterAccountMissing)?;
            let inviter_info = inviter_account.to_account_info();
            require_keys_eq!(inviter_info.key(), expected_key, VaultError::InvalidInviter);
            ensure_referral_account(
                &inviter_info,
                &user_info,
                &system_program_info,
                &[
                    ReferralRecord::SEED,
                    vault.key().as_ref(),
                    referral_record.inviter.as_ref(),
                    &[inviter_bump],
                ],
                ctx.program_id,
            )?;
            let mut record = read_referral(&inviter_info)?;
            if record.user == Pubkey::default() {
                record.user = referral_record.inviter;
                record.vault = vault.key();
            }
            let bonus = (amount as u128)
                .checked_mul(REFERRAL_BONUS_BPS as u128)
                .ok_or(VaultError::MathOverflow)?
                / MAX_BPS as u128;
            record.points_from_invites = record
                .points_from_invites
                .checked_add(bonus)
                .ok_or(VaultError::MathOverflow)?;
            write_referral(&inviter_info, &record)?;
        } else {
            require!(
                ctx.accounts.inviter_record.is_none(),
                VaultError::UnexpectedInviterAccount
            );
        }

        write_referral(&referral_info, &referral_record)?;

        emit!(DepositEvent {
            user: ctx.accounts.user.key(),
            mint: vault_token.mint,
            amount,
            shares,
        });

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);

        let vault_token = &mut ctx.accounts.vault_token;
        let user_position = &mut ctx.accounts.user_position;
        require!(user_position.shares > 0, VaultError::InsufficientShares);

        let clock = Clock::get()?;
        let shares_to_burn = calculate_shares_for_withdraw(amount, vault_token)?;
        require!(shares_to_burn > 0, VaultError::ZeroShares);
        require!(
            user_position.shares >= shares_to_burn,
            VaultError::InsufficientShares
        );

        accrue_points(
            &ctx.accounts.vault,
            vault_token,
            user_position,
            clock.unix_timestamp,
            AccrualSource::Withdraw,
        )?;

        user_position.shares = user_position
            .shares
            .checked_sub(shares_to_burn)
            .ok_or(VaultError::MathOverflow)?;
        vault_token.total_shares = vault_token
            .total_shares
            .checked_sub(shares_to_burn)
            .ok_or(VaultError::MathOverflow)?;
        require!(
            vault_token.total_underlying >= amount,
            VaultError::InvalidAmount
        );
        vault_token.total_underlying = vault_token
            .total_underlying
            .checked_sub(amount)
            .ok_or(VaultError::MathOverflow)?;

        // Transfer tokens out using vault signer
        let vault_key = ctx.accounts.vault.key();
        let seeds = &[
            Vault::SIGNER_SEED,
            vault_key.as_ref(),
            &[ctx.accounts.vault.signer_bump],
        ];
        let signer = &[&seeds[..]];
        let cpi_accounts = Transfer {
            from: ctx.accounts.vault_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.vault_signer.to_account_info(),
        };
        let cpi_ctx =
            CpiContext::new_with_signer(ctx.accounts.token_program.to_account_info(), cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;

        emit!(WithdrawEvent {
            user: ctx.accounts.user.key(),
            mint: vault_token.mint,
            amount,
            shares: shares_to_burn,
        });

        Ok(())
    }

    pub fn harvest(ctx: Context<Harvest>, yield_amount: u64) -> Result<()> {
        require!(yield_amount > 0, VaultError::InvalidAmount);
        let vault = &ctx.accounts.vault;
        require_keys_eq!(
            vault.authority,
            ctx.accounts.keeper.key(),
            VaultError::Unauthorized
        );

        // move realized yield into vault token account
        let cpi_accounts = Transfer {
            from: ctx.accounts.yield_source.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.keeper.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, yield_amount)?;

        ctx.accounts.vault_token.total_underlying = ctx
            .accounts
            .vault_token
            .total_underlying
            .checked_add(yield_amount)
            .ok_or(VaultError::MathOverflow)?;

        ctx.accounts.strategy.last_harvest_ts = Clock::get()?.unix_timestamp;

        emit!(HarvestEvent {
            strategy: ctx.accounts.strategy.key(),
            mint: ctx.accounts.vault_token.mint,
            yield_amount,
        });
        Ok(())
    }
}

fn calculate_shares(amount: u64, vault_token: &VaultToken) -> Result<u64> {
    if vault_token.total_shares == 0 || vault_token.total_underlying == 0 {
        return Ok(amount);
    }
    Ok(((amount as u128)
        .checked_mul(vault_token.total_shares as u128)
        .ok_or(VaultError::MathOverflow)?)
        .checked_div(vault_token.total_underlying as u128)
        .ok_or(VaultError::MathOverflow)? as u64)
}

fn calculate_shares_for_withdraw(amount: u64, vault_token: &VaultToken) -> Result<u64> {
    require!(vault_token.total_underlying > 0, VaultError::InvalidAmount);
    Ok(((amount as u128)
        .checked_mul(vault_token.total_shares as u128)
        .ok_or(VaultError::MathOverflow)?)
        .checked_div(vault_token.total_underlying as u128)
        .ok_or(VaultError::MathOverflow)? as u64)
}

enum AccrualSource {
    Deposit,
    Withdraw,
}

fn accrue_points(
    vault: &Vault,
    vault_token: &VaultToken,
    user_position: &mut UserPosition,
    now: i64,
    _source: AccrualSource,
) -> Result<()> {
    if user_position.user == Pubkey::default() || user_position.shares == 0 {
        user_position.last_points_ts = now;
        return Ok(());
    }
    if vault_token.total_shares == 0 {
        user_position.last_points_ts = now;
        return Ok(());
    }
    let elapsed = now
        .checked_sub(user_position.last_points_ts)
        .ok_or(VaultError::MathOverflow)?;
    if elapsed <= 0 {
        return Ok(());
    }
    let underlying = (user_position.shares as u128)
        .checked_mul(vault_token.total_underlying as u128)
        .ok_or(VaultError::MathOverflow)?
        .checked_div(vault_token.total_shares.max(1) as u128)
        .ok_or(VaultError::MathOverflow)?;
    let mut points = underlying
        .checked_mul(elapsed as u128)
        .ok_or(VaultError::MathOverflow)?
        .checked_mul(vault.base_points_rate as u128)
        .ok_or(VaultError::MathOverflow)?;
    points = points
        .checked_mul(vault_token.points_multiplier_bps as u128)
        .ok_or(VaultError::MathOverflow)?
        / (DAY_IN_SECONDS as u128 * MAX_BPS as u128);
    user_position.cumulative_points = user_position
        .cumulative_points
        .checked_add(points.max(1))
        .ok_or(VaultError::MathOverflow)?;
    user_position.last_points_ts = now;
    Ok(())
}

fn ensure_referral_account<'info>(
    account: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    seeds: &[&[u8]],
    program_id: &Pubkey,
) -> Result<()> {
    if !account.data_is_empty() {
        return Ok(());
    }
    let rent = Rent::get()?.minimum_balance(8 + ReferralRecord::LEN);
    invoke_signed(
        &system_instruction::create_account(
            payer.key,
            account.key,
            rent,
            (8 + ReferralRecord::LEN) as u64,
            program_id,
        ),
        &[payer.clone(), account.clone(), system_program.clone()],
        &[seeds],
    )?;
    write_referral(account, &ReferralRecord::default())?;
    Ok(())
}

fn read_referral(account: &AccountInfo) -> Result<ReferralRecord> {
    let data = account
        .try_borrow_data()
        .map_err(|_| VaultError::AccountSerialization)?;
    let mut cursor: &[u8] = &data;
    Ok(
        ReferralRecord::try_deserialize(&mut cursor)
            .map_err(|_| VaultError::AccountSerialization)?,
    )
}

fn write_referral(account: &AccountInfo, record: &ReferralRecord) -> Result<()> {
    let mut data = account
        .try_borrow_mut_data()
        .map_err(|_| VaultError::AccountSerialization)?;
    let mut cursor = &mut data[..];
    record
        .try_serialize(&mut cursor)
        .map_err(|_| VaultError::AccountSerialization)?;
    Ok(())
}

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(
        init,
        payer = authority,
        seeds = [Vault::SEED, authority.key().as_ref()],
        bump,
        space = 8 + Vault::LEN
    )]
    pub vault: Account<'info, Vault>,
    /// CHECK: signer PDA used only for token authority
    #[account(
        seeds = [Vault::SIGNER_SEED, vault.key().as_ref()],
        bump
    )]
    pub vault_signer: UncheckedAccount<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateVault<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct RegisterToken<'info> {
    #[account(mut, has_one = authority)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub mint: Account<'info, Mint>,
    /// CHECK: PDA acting as token account authority
    #[account(
        seeds = [Vault::SIGNER_SEED, vault.key().as_ref()],
        bump = vault.signer_bump
    )]
    pub vault_signer: UncheckedAccount<'info>,
    #[account(
        init,
        payer = authority,
        seeds = [VaultToken::SEED, vault.key().as_ref(), mint.key().as_ref()],
        bump,
        space = 8 + VaultToken::LEN
    )]
    pub vault_token: Account<'info, VaultToken>,
    #[account(
        init,
        payer = authority,
        associated_token::authority = vault_signer,
        associated_token::mint = mint
    )]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(strategy_id: u8)]
pub struct RegisterStrategy<'info> {
    #[account(has_one = authority)]
    pub vault: Account<'info, Vault>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, has_one = vault)]
    pub vault_token: Account<'info, VaultToken>,
    #[account(
        init,
        payer = authority,
        seeds = [Strategy::SEED, vault_token.key().as_ref(), &[strategy_id]],
        bump,
        space = 8 + Strategy::LEN
    )]
    pub strategy: Account<'info, Strategy>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64, inviter: Pubkey)]
pub struct Deposit<'info> {
    pub vault: Account<'info, Vault>,
    #[account(mut, has_one = vault)]
    pub vault_token: Account<'info, VaultToken>,
    /// CHECK: PDA authority for vault holdings
    #[account(
        seeds = [Vault::SIGNER_SEED, vault.key().as_ref()],
        bump = vault.signer_bump
    )]
    pub vault_signer: UncheckedAccount<'info>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == vault_token.mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = vault_token_account.key() == vault_token.vault_token_account)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(
        seeds = [UserPosition::SEED, vault_token.key().as_ref(), user.key().as_ref()],
        bump,
        init_if_needed,
        payer = user,
        space = 8 + UserPosition::LEN
    )]
    pub user_position: Account<'info, UserPosition>,
    /// CHECK: referral PDA for the depositing user
    #[account(mut)]
    pub referral_record: UncheckedAccount<'info>,
    /// CHECK: optional inviter referral PDA supplied when inviter != default
    #[account(mut)]
    pub inviter_record: Option<UncheckedAccount<'info>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub vault: Account<'info, Vault>,
    #[account(mut, has_one = vault)]
    pub vault_token: Account<'info, VaultToken>,
    /// CHECK: PDA authority
    #[account(
        seeds = [Vault::SIGNER_SEED, vault.key().as_ref()],
        bump = vault.signer_bump
    )]
    pub vault_signer: UncheckedAccount<'info>,
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        seeds = [UserPosition::SEED, vault_token.key().as_ref(), user.key().as_ref()],
        bump = user_position.bump
    )]
    pub user_position: Account<'info, UserPosition>,
    #[account(
        mut,
        constraint = user_token_account.owner == user.key(),
        constraint = user_token_account.mint == vault_token.mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut, constraint = vault_token_account.key() == vault_token.vault_token_account)]
    pub vault_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Harvest<'info> {
    pub vault: Account<'info, Vault>,
    #[account(mut, has_one = vault)]
    pub vault_token: Account<'info, VaultToken>,
    #[account(
        mut,
        constraint = strategy.vault_token == vault_token.key()
    )]
    pub strategy: Account<'info, Strategy>,
    #[account(mut, constraint = vault_token_account.key() == vault_token.vault_token_account)]
    pub vault_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub yield_source: Account<'info, TokenAccount>,
    pub keeper: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Vault {
    pub authority: Pubkey,
    pub bump: u8,
    pub signer_bump: u8,
    pub base_points_rate: u64,
    pub paused: bool,
}

impl Vault {
    pub const LEN: usize = 32 + 1 + 1 + 8 + 1 + 7;
    pub const SEED: &'static [u8] = b"vault";
    pub const SIGNER_SEED: &'static [u8] = b"vault_signer";
}

#[account]
pub struct VaultToken {
    pub vault: Pubkey,
    pub mint: Pubkey,
    pub vault_token_account: Pubkey,
    pub total_underlying: u64,
    pub total_shares: u64,
    pub points_multiplier_bps: u16,
    pub bump: u8,
}

impl VaultToken {
    pub const LEN: usize = 32 + 32 + 32 + 8 + 8 + 2 + 1 + 7;
    pub const SEED: &'static [u8] = b"vault_token";
}

#[account]
pub struct Strategy {
    pub vault_token: Pubkey,
    pub authority: Pubkey,
    pub strategy_id: u8,
    pub weight_bps: u16,
    pub last_harvest_ts: i64,
}

impl Strategy {
    pub const LEN: usize = 32 + 32 + 1 + 2 + 8 + 15;
    pub const SEED: &'static [u8] = b"strategy";
}

#[account]
pub struct UserPosition {
    pub vault_token: Pubkey,
    pub user: Pubkey,
    pub shares: u64,
    pub cumulative_points: u128,
    pub last_points_ts: i64,
    pub bump: u8,
}

impl UserPosition {
    pub const LEN: usize = 32 + 32 + 8 + 16 + 8 + 1 + 7;
    pub const SEED: &'static [u8] = b"user_position";
}

#[account]
pub struct ReferralRecord {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub inviter: Pubkey,
    pub points_from_invites: u128,
    pub bump: u8,
}

impl ReferralRecord {
    pub const LEN: usize = 32 + 32 + 32 + 16 + 1 + 7;
    pub const SEED: &'static [u8] = b"referral";

    pub fn pda(vault: &Pubkey, user: &Pubkey, program_id: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[Self::SEED, vault.as_ref(), user.as_ref()], program_id)
    }
}

impl Default for ReferralRecord {
    fn default() -> Self {
        Self {
            vault: Pubkey::default(),
            user: Pubkey::default(),
            inviter: Pubkey::default(),
            points_from_invites: 0,
            bump: 0,
        }
    }
}

#[event]
pub struct DepositEvent {
    pub user: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct WithdrawEvent {
    pub user: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub shares: u64,
}

#[event]
pub struct HarvestEvent {
    pub strategy: Pubkey,
    pub mint: Pubkey,
    pub yield_amount: u64,
}

#[event]
pub struct TokenRegistered {
    pub mint: Pubkey,
    pub multiplier_bps: u16,
}

#[event]
pub struct StrategyRegistered {
    pub vault_token: Pubkey,
    pub strategy: Pubkey,
    pub weight_bps: u16,
}

#[error_code]
pub enum VaultError {
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Invalid multiplier")]
    InvalidMultiplier,
    #[msg("Invalid amount")]
    InvalidAmount,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Vault is paused")]
    VaultPaused,
    #[msg("Zero shares")]
    ZeroShares,
    #[msg("Insufficient shares")]
    InsufficientShares,
    #[msg("Invalid inviter")]
    InvalidInviter,
    #[msg("Inviter already set")]
    InviterLocked,
    #[msg("Missing inviter record")]
    InviterAccountMissing,
    #[msg("Unexpected inviter account")]
    UnexpectedInviterAccount,
    #[msg("Failed to serialize account data")]
    AccountSerialization,
    #[msg("Referral record PDA mismatch")]
    InvalidReferralAccount,
}


use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer, Mint};
use solana_program::pubkey;
use solana_program::program::invoke_signed;
use spl_token;

declare_id!("2KL4BKvUtDmABvuvRopkCEb33myWM1W9BGodAZ82RWDT");

// Price oracle PDA seeds
const PRICE_ORACLE_SEED: &[u8] = b"price_oracle";

// Static escrow wallet address
pub const ESCROW_WALLET_SEED: &[u8] = b"escrow_wallet";

#[program]
pub mod usdfg_smart_contract {
    use super::*;

    // Minimum and maximum entry fees in USDFG tokens
    const MIN_ENTRY_FEE_USDFG: u64 = 1;  // 1 USDFG minimum
    const MAX_ENTRY_FEE_USDFG: u64 = 1000; // 1000 USDFG maximum

    pub fn initialize(ctx: Context<Initialize>, admin: Pubkey) -> Result<()> {
        let admin_state = &mut ctx.accounts.admin_state;
        admin_state.admin = admin;
        admin_state.is_active = true;
        admin_state.created_at = Clock::get()?.unix_timestamp;
        admin_state.last_updated = Clock::get()?.unix_timestamp;
        
        emit!(AdminInitialized {
            admin: admin,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn initialize_price_oracle(ctx: Context<InitializePriceOracle>) -> Result<()> {
        require!(
            ctx.accounts.admin_state.admin == ctx.accounts.admin.key(),
            ChallengeError::InvalidAdmin
        );

        let price_oracle = &mut ctx.accounts.price_oracle;
        price_oracle.price = 1000; // Default price of $10.00 (1000 cents)
        price_oracle.last_updated = Clock::get()?.unix_timestamp;
        Ok(())
    }

    pub fn update_admin(ctx: Context<UpdateAdmin>, new_admin: Pubkey) -> Result<()> {
        let admin_state = &mut ctx.accounts.admin_state;
        
        // Security: Verify current admin
        require!(
            admin_state.admin == ctx.accounts.current_admin.key(),
            ChallengeError::Unauthorized
        );
        
        // Security: Verify admin state is active
        require!(
            admin_state.is_active,
            ChallengeError::AdminInactive
        );

        let old_admin = admin_state.admin;
        admin_state.admin = new_admin;
        admin_state.last_updated = Clock::get()?.unix_timestamp;

        emit!(AdminUpdated {
            old_admin: old_admin,
            new_admin: new_admin,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn revoke_admin(ctx: Context<RevokeAdmin>) -> Result<()> {
        let admin_state = &mut ctx.accounts.admin_state;
        
        // Security: Verify current admin
        require!(
            admin_state.admin == ctx.accounts.current_admin.key(),
            ChallengeError::Unauthorized
        );
        
        // Security: Verify admin state is active
        require!(
            admin_state.is_active,
            ChallengeError::AdminInactive
        );

        admin_state.is_active = false;
        admin_state.last_updated = Clock::get()?.unix_timestamp;

        emit!(AdminRevoked {
            admin: admin_state.admin,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn update_price(ctx: Context<UpdatePrice>, price: u64) -> Result<()> {
        require!(
            ctx.accounts.admin_state.admin == ctx.accounts.admin.key(),
            ChallengeError::InvalidAdmin
        );

        let price_oracle = &mut ctx.accounts.price_oracle;
        price_oracle.price = price;
        price_oracle.last_updated = Clock::get()?.unix_timestamp;
        Ok(())
    }

    pub fn create_challenge(ctx: Context<CreateChallenge>, usdfg_amount: u64) -> Result<()> {
        // Validate entry fee limits
        require!(
            usdfg_amount >= MIN_ENTRY_FEE_USDFG,
            ChallengeError::EntryFeeTooLow
        );
        require!(
            usdfg_amount <= MAX_ENTRY_FEE_USDFG,
            ChallengeError::EntryFeeTooHigh
        );
        
        // ✅ REMOVED: Oracle freshness check (was blocking regular users)
        // Oracle check completely removed - not needed for USDFG native token
        
        // Set dispute_timer to now + 900 seconds (15 minutes)
        let now = Clock::get()?.unix_timestamp;
        let dispute_timer = now + 900;
        let challenge = &mut ctx.accounts.challenge;
        
        // Transfer tokens to escrow
        let cpi_accounts = Transfer {
            from: ctx.accounts.creator_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.creator.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::transfer(cpi_ctx, usdfg_amount)?;
        
        // Initialize challenge
        challenge.creator = ctx.accounts.creator.key();
        challenge.challenger = None;
        challenge.entry_fee = usdfg_amount;
        challenge.status = ChallengeStatus::Open;
        challenge.created_at = now;
        challenge.last_updated = now;
        challenge.processing = false;
        challenge.dispute_timer = dispute_timer;
        
        emit!(ChallengeCreated {
            creator: challenge.creator,
            amount: challenge.entry_fee,
            timestamp: challenge.created_at,
        });
        
        Ok(())
    }

    pub fn accept_challenge(ctx: Context<AcceptChallenge>) -> Result<()> {
        // Security: Verify admin state is active
        require!(
            ctx.accounts.admin_state.is_active,
            ChallengeError::AdminInactive
        );

        let challenge = &mut ctx.accounts.challenge;
        
        // Security: Verify challenge is open
        require!(challenge.status == ChallengeStatus::Open, ChallengeError::NotOpen);
        
        // Security: Prevent self-challenge
        require!(challenge.creator != ctx.accounts.challenger.key(), ChallengeError::SelfChallenge);
        
        // Security: Verify challenger has enough tokens
        require!(
            ctx.accounts.challenger_token_account.amount >= challenge.entry_fee,
            ChallengeError::InsufficientFunds
        );
        
        // Security: Verify challenge hasn't expired
        require!(
            Clock::get()?.unix_timestamp < challenge.dispute_timer,
            ChallengeError::ChallengeExpired
        );

        challenge.challenger = Some(ctx.accounts.challenger.key());
        challenge.status = ChallengeStatus::InProgress;
        challenge.last_updated = Clock::get()?.unix_timestamp;

        // Transfer tokens to escrow
        let cpi_accounts = Transfer {
            from: ctx.accounts.challenger_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            authority: ctx.accounts.challenger.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::transfer(cpi_ctx, challenge.entry_fee)?;

        emit!(ChallengeAccepted {
            challenge: challenge.key(),
            challenger: ctx.accounts.challenger.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn resolve_challenge(ctx: Context<ResolveChallenge>, winner: Pubkey) -> Result<()> {
        let challenge = &mut ctx.accounts.challenge;
        require!(!challenge.processing, ChallengeError::ReentrancyDetected);
        challenge.processing = true;
        
        // Security checks...
        require!(
            ctx.accounts.admin_state.is_active,
            ChallengeError::AdminInactive
        );
        require!(challenge.status == ChallengeStatus::InProgress, ChallengeError::NotInProgress);
        require!(
            winner == challenge.creator || winner == challenge.challenger.unwrap(),
            ChallengeError::InvalidWinner
        );
        require!(
            Clock::get()?.unix_timestamp < challenge.dispute_timer,
            ChallengeError::ChallengeExpired
        );

        challenge.status = ChallengeStatus::Completed;
        challenge.winner = Some(winner);
        challenge.last_updated = Clock::get()?.unix_timestamp;

        // Transfer all tokens to winner using PDA signing
        let escrow_seeds = [
            ESCROW_WALLET_SEED,
            challenge.to_account_info().key.as_ref(),
            ctx.accounts.mint.to_account_info().key.as_ref(),
            &[*ctx.bumps.get("escrow_token_account").unwrap()]
        ];
        let signer_seeds = [&escrow_seeds[..]];

        let cpi_accounts = Transfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.winner_token_account.to_account_info(),
            authority: ctx.accounts.escrow_wallet.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            &signer_seeds
        );
        token::transfer(cpi_ctx, challenge.entry_fee * 2)?;

        emit!(PayoutCompleted {
            challenge: challenge.key(),
            winner,
            amount: challenge.entry_fee * 2,
            timestamp: challenge.last_updated,
        });
        challenge.processing = false;
        Ok(())
    }

    pub fn cancel_challenge(ctx: Context<CancelChallenge>) -> Result<()> {
        let challenge = &mut ctx.accounts.challenge;
        require!(!challenge.processing, ChallengeError::ReentrancyDetected);
        challenge.processing = true;
        
        // Security: Verify admin state is active
        require!(
            ctx.accounts.admin_state.is_active,
            ChallengeError::AdminInactive
        );
        require!(challenge.status == ChallengeStatus::Open, ChallengeError::NotOpen);
        require!(
            ctx.accounts.creator.key() == challenge.creator,
            ChallengeError::Unauthorized
        );
        require!(
            Clock::get()?.unix_timestamp < challenge.dispute_timer,
            ChallengeError::ChallengeExpired
        );
        
        challenge.status = ChallengeStatus::Cancelled;
        challenge.last_updated = Clock::get()?.unix_timestamp;
        
        // Return tokens to creator
        let cpi_accounts = Transfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: ctx.accounts.escrow_wallet.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::transfer(cpi_ctx, challenge.entry_fee)?;
        
        emit!(RefundIssued {
            challenge: challenge.key(),
            recipient: challenge.creator,
            amount: challenge.entry_fee,
            timestamp: challenge.last_updated,
        });
        challenge.processing = false;
        Ok(())
    }

    pub fn claim_refund(ctx: Context<CancelChallenge>) -> Result<()> {
        let challenge = &mut ctx.accounts.challenge;
        require!(!challenge.processing, ChallengeError::ReentrancyDetected);
        challenge.processing = true;
        
        // Only creator can claim
        require!(ctx.accounts.creator.key() == challenge.creator, ChallengeError::Unauthorized);
        // Only if open and expired
        require!(challenge.status == ChallengeStatus::Open, ChallengeError::NotOpen);
        require!(Clock::get()?.unix_timestamp >= challenge.dispute_timer, ChallengeError::ChallengeNotExpired);
        // No challenger
        require!(challenge.challenger.is_none(), ChallengeError::AlreadyAccepted);
        
        challenge.status = ChallengeStatus::Cancelled;
        challenge.last_updated = Clock::get()?.unix_timestamp;
        
        // Return tokens to creator
        let cpi_accounts = Transfer {
            from: ctx.accounts.escrow_token_account.to_account_info(),
            to: ctx.accounts.creator_token_account.to_account_info(),
            authority: ctx.accounts.escrow_wallet.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );
        token::transfer(cpi_ctx, challenge.entry_fee)?;
        
        emit!(RefundIssued {
            challenge: challenge.key(),
            recipient: challenge.creator,
            amount: challenge.entry_fee,
            timestamp: challenge.last_updated,
        });
        challenge.processing = false;
        Ok(())
    }

    pub fn dispute_challenge(ctx: Context<DisputeChallenge>) -> Result<()> {
        // Security: Verify admin state is active
        require!(
            ctx.accounts.admin_state.is_active,
            ChallengeError::AdminInactive
        );

        let challenge = &mut ctx.accounts.challenge;
        
        // Security: Verify challenge is in progress
        require!(challenge.status == ChallengeStatus::InProgress, ChallengeError::NotInProgress);
        
        // Security: Verify challenge has expired
        require!(
            Clock::get()?.unix_timestamp >= challenge.dispute_timer,
            ChallengeError::ChallengeNotExpired
        );
        
        // Security: Verify disputer is either creator or challenger
        require!(
            ctx.accounts.disputer.key() == challenge.creator || 
            ctx.accounts.disputer.key() == challenge.challenger.unwrap(),
            ChallengeError::Unauthorized
        );

        challenge.status = ChallengeStatus::Disputed;
        challenge.last_updated = Clock::get()?.unix_timestamp;

        emit!(ChallengeDisputed {
            challenge: challenge.key(),
            disputer: ctx.accounts.disputer.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = payer,
        space = AdminState::LEN,
        seeds = [b"admin"],
        bump
    )]
    pub admin_state: Account<'info, AdminState>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateAdmin<'info> {
    #[account(mut)]
    pub admin_state: Account<'info, AdminState>,
    #[account(mut)]
    pub current_admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct RevokeAdmin<'info> {
    #[account(mut)]
    pub admin_state: Account<'info, AdminState>,
    #[account(mut)]
    pub current_admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdatePrice<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"admin"],
        bump
    )]
    pub admin_state: Account<'info, AdminState>,

    #[account(
        mut,
        seeds = [PRICE_ORACLE_SEED],
        bump
    )]
    pub price_oracle: Account<'info, PriceOracle>,
}

#[account]
pub struct AdminState {
    pub admin: Pubkey,
    pub is_active: bool,
    pub created_at: i64,
    pub last_updated: i64,
}

impl AdminState {
    pub const LEN: usize = 8 + // discriminator
        32 + // admin
        1 + // is_active
        8 + // created_at
        8; // last_updated
}

// ✅ FIXED: Removed oracle accounts from CreateChallenge
#[derive(Accounts)]
#[instruction(entry_fee: u64)]
pub struct CreateChallenge<'info> {
    #[account(
        init,
        payer = creator,
        space = Challenge::LEN,
        seeds = [b"challenge", creator.key().as_ref(), challenge_seed.key().as_ref()],
        bump
    )]
    pub challenge: Account<'info, Challenge>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut, constraint = creator_token_account.owner == creator.key())]
    pub creator_token_account: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = creator,
        seeds = [ESCROW_WALLET_SEED, challenge.key().as_ref(), mint.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = escrow_wallet
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,
    /// CHECK: This is the escrow wallet that holds the tokens
    pub escrow_wallet: AccountInfo<'info>,
    pub challenge_seed: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
    // ✅ REMOVED: Oracle accounts - no longer needed for challenge creation
    pub mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct AcceptChallenge<'info> {
    #[account(mut)]
    pub challenge: Account<'info, Challenge>,
    #[account(mut)]
    pub challenger: Signer<'info>,
    #[account(
        mut,
        constraint = challenger_token_account.owner == challenger.key()
    )]
    pub challenger_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [ESCROW_WALLET_SEED, challenge.key().as_ref(), mint.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = escrow_wallet
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub admin_state: Account<'info, AdminState>,
    /// CHECK: This is the escrow wallet that holds the tokens
    #[account(
        seeds = [ESCROW_WALLET_SEED],
        bump
    )]
    pub escrow_wallet: AccountInfo<'info>,
    pub mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct ResolveChallenge<'info> {
    #[account(mut)]
    pub challenge: Account<'info, Challenge>,
    #[account(
        mut,
        seeds = [ESCROW_WALLET_SEED, challenge.key().as_ref(), mint.key().as_ref()],
        bump,
        token::mint = mint,
        token::authority = escrow_wallet
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = winner_token_account.mint == mint.key()
    )]
    pub winner_token_account: Account<'info, TokenAccount>,
    /// CHECK: This is the escrow wallet that holds the tokens
    #[account(
        seeds = [ESCROW_WALLET_SEED],
        bump
    )]
    pub escrow_wallet: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub admin_state: Account<'info, AdminState>,
    pub mint: Account<'info, Mint>,
}

#[derive(Accounts)]
pub struct CancelChallenge<'info> {
    #[account(mut)]
    pub challenge: Account<'info, Challenge>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        mut,
        constraint = creator_token_account.owner == creator.key(),
        constraint = creator_token_account.mint == escrow_token_account.mint
    )]
    pub creator_token_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = escrow_token_account.owner == escrow_wallet.key()
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,
    /// CHECK: This is the escrow wallet that holds the tokens
    #[account(
        seeds = [ESCROW_WALLET_SEED],
        bump
    )]
    pub escrow_wallet: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub admin_state: Account<'info, AdminState>,
}

#[derive(Accounts)]
pub struct DisputeChallenge<'info> {
    #[account(mut)]
    pub challenge: Account<'info, Challenge>,
    #[account(mut)]
    pub disputer: Signer<'info>,
    pub admin_state: Account<'info, AdminState>,
}

#[account]
pub struct Challenge {
    pub creator: Pubkey,
    pub challenger: Option<Pubkey>,
    pub entry_fee: u64,
    pub status: ChallengeStatus,
    pub dispute_timer: i64,
    pub winner: Option<Pubkey>,
    pub created_at: i64,
    pub last_updated: i64,
    pub processing: bool, // reentrancy protection
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeStatus {
    Open,
    InProgress,
    Completed,
    Cancelled,
    Disputed,
}

impl Challenge {
    pub const LEN: usize = 8 + // discriminator
        32 + // creator
        1 + 32 + // challenger (Option<Pubkey>)
        8 + // entry_fee
        1 + // status
        8 + // dispute_timer
        1 + 32 + // winner (Option<Pubkey>)
        8 + // created_at
        8 + // last_updated
        1; // processing
}

#[error_code]
pub enum ChallengeError {
    #[msg("Challenge is not open")]
    NotOpen,
    #[msg("Challenge is not in progress")]
    NotInProgress,
    #[msg("Cannot challenge yourself")]
    SelfChallenge,
    #[msg("Invalid winner")]
    InvalidWinner,
    #[msg("Insufficient funds")]
    InsufficientFunds,
    #[msg("Invalid escrow wallet")]
    InvalidEscrowWallet,
    #[msg("Challenge has expired")]
    ChallengeExpired,
    #[msg("Challenge not expired")]
    ChallengeNotExpired,
    #[msg("Entry fee too low")]
    EntryFeeTooLow,
    #[msg("Entry fee too high")]
    EntryFeeTooHigh,
    #[msg("Invalid token mint")]
    InvalidTokenMint,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Admin is inactive")]
    AdminInactive,
    #[msg("Invalid admin")]
    InvalidAdmin,
    // ✅ REMOVED: StaleOraclePrice error - no longer needed
    #[msg("Reentrancy detected")] 
    ReentrancyDetected,
    #[msg("Challenge already accepted")]
    AlreadyAccepted,
}

#[event]
pub struct ChallengeCreated {
    pub creator: Pubkey,
    pub amount: u64,       // Amount in USDFG tokens
    pub timestamp: i64,
}

#[event]
pub struct ChallengeAccepted {
    pub challenge: Pubkey,
    pub challenger: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ChallengeResolved {
    pub challenge: Pubkey,
    pub winner: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct ChallengeCancelled {
    pub challenge: Pubkey,
    pub creator: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ChallengeDisputed {
    pub challenge: Pubkey,
    pub disputer: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdminInitialized {
    pub admin: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdminUpdated {
    pub old_admin: Pubkey,
    pub new_admin: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdminRevoked {
    pub admin: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct PayoutCompleted {
    pub challenge: Pubkey,
    pub winner: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct RefundIssued {
    pub challenge: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[account]
pub struct PriceOracle {
    pub price: u64,        // Price in cents (e.g., 1000 = $10.00)
    pub last_updated: i64,
}

#[derive(Accounts)]
pub struct InitializePriceOracle<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [b"admin"],
        bump
    )]
    pub admin_state: Account<'info, AdminState>,

    #[account(
        init,
        payer = admin,
        space = 8 + 8 + 8, // discriminator + price + last_updated
        seeds = [PRICE_ORACLE_SEED],
        bump
    )]
    pub price_oracle: Account<'info, PriceOracle>,

    pub system_program: Program<'info, System>,
}

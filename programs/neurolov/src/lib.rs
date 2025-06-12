use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, Transfer as TokenTransfer},
};
use anchor_lang::solana_program::{program::invoke, system_instruction};

pub mod constant;
use constant::*;

declare_id!("Duf9UdBXfrxgBeZgZ2DUxRgFSZ4qCzEgGyxFmuQHGHZH");

#[program]
pub mod presale {
    use super::*;

    /// Initializes the presale contract with specified parameters.
    /// This function sets up the admin, prices, sale durations, hardcap, and wallet accounts.
    pub fn initialize(
        ctx: Context<Initialize>,
        usd_price_cents_per_nlov: u64,
        sol_price_lamports_per_nlov: u64,
        private_sale_duration_days: i64,
        public_sale_duration_days: i64,
        hardcap_tokens: u64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale;

        let bump = ctx.bumps.presale;

        presale.admin = ctx.accounts.admin.key();
        presale.usd_price_cents_per_nlov = usd_price_cents_per_nlov;
        presale.sol_price_lamports_per_nlov = sol_price_lamports_per_nlov;
        presale.presale_start = Clock::get()?.unix_timestamp;
        presale.private_sale_duration = private_sale_duration_days * 86400;
        presale.public_sale_duration = public_sale_duration_days * 86400;
        presale.sale_stage = 0;
        presale.total_sold = 0;
        presale.pool_created = false;
        presale.hardcap_tokens = hardcap_tokens;
        presale.presale_wallet = ctx.accounts.presale_wallet.key();
        presale.merchant_wallet = ctx.accounts.merchant_wallet.key();
        presale.bump = bump;

        msg!(
            "Presale contract initialized! USD Price: {} cents/NLOV, SOL Price: {} lamports/NLOV, Private Duration: {} days, Public Duration: {} days, Hardcap Tokens: {}",
            usd_price_cents_per_nlov,
            sol_price_lamports_per_nlov,
            private_sale_duration_days,
            public_sale_duration_days,
            hardcap_tokens,
        );

        Ok(())
    }

    /// Advances the sale stage of the presale contract.
    /// Stages: 0 (Not Started) -> 1 (Private Sale) -> 2 (Public Sale) -> 3 (Ended).
    /// Requires the admin to perform this action and checks sale duration.
    pub fn set_stage(ctx: Context<SetStage>) -> Result<()> {
        let presale = &mut ctx.accounts.presale;

        require!(
            presale.admin == ctx.accounts.admin.key(),
            PresaleError::Unauthorized
        );

        let clock = Clock::get()?;

        match presale.sale_stage {
            0 => {
                presale.presale_start = clock.unix_timestamp;
                presale.sale_stage = 1;
                msg!("Private sale started at {}", presale.presale_start);
            }
            1 => {
                require!(
                    clock.unix_timestamp >= presale.presale_start + presale.private_sale_duration,
                    PresaleError::PrivateSaleNotOver
                );
                presale.sale_stage = 2;
                msg!("Public sale started at {}", clock.unix_timestamp);
            }
            2 => {
       
                require!(
                    clock.unix_timestamp
                        >= presale.presale_start
                            + presale.private_sale_duration
                            + presale.public_sale_duration,
                    PresaleError::PublicSaleNotOver
                );
                presale.sale_stage = 3;
                msg!("Presale ended at {}", clock.unix_timestamp);
            }
            _ => {
                return Err(PresaleError::SaleAlreadyEnded.into());
            }
        }

        Ok(())
    }

    pub fn update_sale_period(
        ctx: Context<UpdateSalePeriod>,
        new_private_sale_duration_days: i64,
        new_public_sale_duration_days: i64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale;

        require!(
            presale.admin == ctx.accounts.admin.key(),
            PresaleError::Unauthorized
        );

        require!(presale.sale_stage < 3, PresaleError::SaleAlreadyEnded);

        presale.private_sale_duration = new_private_sale_duration_days * 86400;
        presale.public_sale_duration = new_public_sale_duration_days * 86400;

        msg!(
            "Updated sale period: Private Sale = {} days, Public Sale = {} days",
            new_private_sale_duration_days,
            new_public_sale_duration_days
        );

        Ok(())
    }

    /// Allows a buyer to purchase tokens using SOL.
    /// The function supports Web3 (on-chain SOL transfer) and Web2 
    /// Calculates tokens based on SOL amount and current price, updates total_sold.
    pub fn buy_tokens(
        ctx: Context<BuyTokens>,
        payment_type: u8,
        lamports_sent: u64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale;
        let buyer = &ctx.accounts.buyer;
        let token_decimals = ctx.accounts.token_mint.decimals;

        require!(
            presale.sale_stage == 1 || presale.sale_stage == 2,
            PresaleError::PresaleNotActive
        );

        let tokens_to_purchase_user_units = lamports_sent
            .checked_div(presale.sol_price_lamports_per_nlov)
            .ok_or(PresaleError::InvalidPrice)?;

        require!(tokens_to_purchase_user_units >= 1, PresaleError::InvalidPrice);

        let tokens_to_purchase_raw =
            tokens_to_purchase_user_units.checked_mul(10u64.pow(token_decimals as u32)).unwrap();

        require!(
            presale
                .total_sold
                .checked_add(tokens_to_purchase_raw)
                .unwrap_or(u64::MAX)
                <= presale.hardcap_tokens,
            PresaleError::HardcapReached
        );

        let available_presale_tokens_raw = ctx.accounts.presale_wallet.amount;
        let tokens_currently_sold_raw = presale.total_sold;

        require!(
            available_presale_tokens_raw.saturating_sub(tokens_currently_sold_raw)
                >= tokens_to_purchase_raw,
            PresaleError::InsufficientTokens
        );

        if payment_type == 0 {
            invoke(
                &system_instruction::transfer(
                    &buyer.key(),
                    &presale.merchant_wallet,
                    lamports_sent,
                ),
                &[
                    ctx.accounts.buyer.to_account_info(),
                    ctx.accounts.merchant_wallet.to_account_info(),
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
        } else if payment_type == 1 {
            msg!(
                "Web2 payment type selected. Assuming off-chain SOL payment of {} lamports.",
                lamports_sent
            );
        } else {
            return Err(PresaleError::InvalidPaymentType.into());
        }

        presale.total_sold = presale
            .total_sold
            .checked_add(tokens_to_purchase_raw)
            .unwrap();

        emit!(BuyTokensEvent {
            buyer: buyer.key(),
            tokens_purchased: tokens_to_purchase_user_units,
            sol_spent: lamports_sent,
            sol_price_lamports_per_nlov: presale.sol_price_lamports_per_nlov,
            payment_type,
        });

        msg!(
            "Buyer {} purchased {} tokens for {} lamports using payment_type: {}",
            buyer.key(),
            tokens_to_purchase_user_units,
            lamports_sent,
            payment_type
        );

        Ok(())
    }

    pub fn check_presale_token_balance(ctx: Context<CheckPresaleTokenBalance>) -> Result<u64> {
        let presale = &ctx.accounts.presale;
        let token_decimals = ctx.accounts.token_mint.decimals;
        let available_tokens_raw = ctx.accounts.presale_wallet.amount;
        let remaining_tokens_raw = available_tokens_raw.saturating_sub(presale.total_sold);
        let remaining_tokens_user_units = remaining_tokens_raw / 10u64.pow(token_decimals as u32);

        msg!(
            "Available presale tokens: {} (raw: {})",
            remaining_tokens_user_units,
            remaining_tokens_raw
        );

        Ok(remaining_tokens_user_units)
    }

    pub fn update_sale_price(ctx: Context<UpdateSalePrice>, new_usd_price_cents: u64, new_sol_price_lamports: u64) -> Result<()> {
        let presale = &mut ctx.accounts.presale;

        require!(
            presale.admin == ctx.accounts.admin.key(),
            PresaleError::Unauthorized
        );

        require!(
            presale.sale_stage == 1 || presale.sale_stage == 2,
            PresaleError::PresaleNotActive
        );

        presale.usd_price_cents_per_nlov = new_usd_price_cents;
        presale.sol_price_lamports_per_nlov = new_sol_price_lamports;

        emit!(UpdateSalePriceEvent {
            admin: ctx.accounts.admin.key(),
            new_usd_price_cents,
            new_sol_price_lamports,
            sale_stage: presale.sale_stage,
        });

        msg!(
            "Sale price updated to {} cents/NLOV (USD) and {} lamports/NLOV (SOL) for stage {}",
            new_usd_price_cents,
            new_sol_price_lamports,
            presale.sale_stage
        );

        Ok(())
    }

    pub fn buy_tokens_by_stable_coin(
        ctx: Context<BuyTokensByStableCoin>,
        payment_type: u8,
        stable_coin_amount_user_units: u64,
    ) -> Result<()> {
        let presale = &mut ctx.accounts.presale;
        let buyer = &ctx.accounts.buyer;
        let token_decimals = ctx.accounts.token_mint.decimals;
        let stable_coin_decimals = ctx.accounts.stable_coin_mint.decimals;

        require!(
            ctx.accounts.stable_coin_mint.key() == USDC_ADDRESS
                || ctx.accounts.stable_coin_mint.key() == USDT_ADDRESS,
            PresaleError::InvalidStableToken
        );

        require!(
            stable_coin_amount_user_units >= 1,
            PresaleError::InvalidPrice
        );

        require!(
            presale.sale_stage == 1 || presale.sale_stage == 2,
            PresaleError::PresaleNotActive
        );

        let stable_coin_amount_raw =
            stable_coin_amount_user_units.checked_mul(10u64.pow(stable_coin_decimals as u32)).unwrap();

        let stable_coin_amount_cents = stable_coin_amount_user_units.checked_mul(100).unwrap();

        let tokens_to_purchase_user_units = stable_coin_amount_cents
            .checked_div(presale.usd_price_cents_per_nlov)
            .ok_or(PresaleError::InvalidPrice)?;

        let tokens_to_purchase_raw =
            tokens_to_purchase_user_units.checked_mul(10u64.pow(token_decimals as u32)).unwrap();

        require!(
            presale
                .total_sold
                .checked_add(tokens_to_purchase_raw)
                .unwrap_or(u64::MAX)
                <= presale.hardcap_tokens,
            PresaleError::HardcapReached
        );

        let available_presale_tokens_raw = ctx.accounts.presale_wallet.amount;
        let tokens_currently_sold_raw = presale.total_sold;

        require!(
            available_presale_tokens_raw.saturating_sub(tokens_currently_sold_raw)
                >= tokens_to_purchase_raw,
            PresaleError::InsufficientTokens
        );

        if payment_type == 0 {
            token::transfer(
                CpiContext::new(
                    ctx.accounts.token_program.to_account_info(),
                    TokenTransfer {
                        from: ctx.accounts.buyer_stable_coin_account.to_account_info(),
                        to: ctx.accounts.merchant_stable_coin_account.to_account_info(),
                        authority: ctx.accounts.buyer.to_account_info(),
                    },
                ),
                stable_coin_amount_raw,
            )?;
        } else if payment_type == 1 {
            msg!("Web2 payment type selected. Assuming off-chain stablecoin payment of {} (raw: {}).", stable_coin_amount_user_units, stable_coin_amount_raw);
        } else {
            return Err(PresaleError::InvalidPaymentType.into());
        }

        presale.total_sold = presale
            .total_sold
            .checked_add(tokens_to_purchase_raw)
            .unwrap();

        emit!(BuyTokensByStableCoinEvent {
            buyer: buyer.key(),
            tokens_purchased: tokens_to_purchase_user_units,
            stable_coin_amount: stable_coin_amount_user_units,
            payment_type,
        });

        let stable_coin_symbol = if ctx.accounts.stable_coin_mint.key() == USDC_ADDRESS {
            "USDC"
        } else {
            "USDT"
        };
        msg!(
            "Buyer {} purchased {} tokens with {} {} (raw: {}) using payment_type: {}",
            buyer.key(),
            tokens_to_purchase_user_units,
            stable_coin_amount_user_units,
            stable_coin_symbol,
            stable_coin_amount_raw,
            payment_type
        );

        Ok(())
    }

    /// Finalizes the presale by transferring any unsold tokens from the presale wallet
    /// to a designated liquidity wallet.
    /// This can only be done by the admin after the sale has ended and before a liquidity pool is created.
    pub fn finalize_presale(ctx: Context<FinalizePresale>) -> Result<()> {
        let presale = &mut ctx.accounts.presale;
        let admin_key = ctx.accounts.admin.key();
        let bump = ctx.bumps.presale;
        let token_decimals = ctx.accounts.token_mint.decimals; // Get actual token decimals

        require!(presale.admin == admin_key, PresaleError::Unauthorized);

        require!(presale.sale_stage == 3, PresaleError::PresaleActive);

        require!(
            !presale.pool_created,
            PresaleError::LiquidityPoolAlreadyCreated
        );

        // Calculate unsold presale tokens in raw units
        let available_presale_tokens_raw = ctx.accounts.presale_wallet.amount;
        let unsold_presale_tokens_raw =
            available_presale_tokens_raw.saturating_sub(presale.total_sold);

        let seeds: &[&[u8]] = &[PRESALE_SEED, admin_key.as_ref(), &[bump]];
        let signer_seeds: &[&[&[u8]]] = &[&seeds[..]];

        // Transfer unsold presale tokens to liquidity wallet if any exist
        if unsold_presale_tokens_raw > 0 {
            token::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    TokenTransfer {
                        from: ctx.accounts.presale_wallet.to_account_info(),
                        to: ctx.accounts.liquidity_wallet.to_account_info(),
                        authority: presale.to_account_info(),
                    },
                    signer_seeds,
                ),
                unsold_presale_tokens_raw,
            )?;
        }

        presale.pool_created = true;

        emit!(FinalizePresaleEvent {
            admin: ctx.accounts.admin.key(),
            unsold_presale_tokens: unsold_presale_tokens_raw / 10u64.pow(token_decimals as u32), // Emit user-facing units
        });

        msg!(
            "Presale finalized! {} unsold presale tokens moved to liquidity wallet.",
            unsold_presale_tokens_raw / 10u64.pow(token_decimals as u32),
        );

        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(usd_price_cents_per_nlov: u64, sol_price_lamports_per_nlov: u64, private_sale_duration_days: i64, public_sale_duration_days: i64, hardcap_tokens: u64)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        seeds = [PRESALE_SEED, admin.key().as_ref()],
        bump,
        space = 8 + 32 + 8 + 8 + 8 + 8 + 8 + 1 + 8 + 1 + 32 + 32 + 8 + 1
    )]
    pub presale: Account<'info, Presale>,
    pub token_mint: Account<'info, Mint>,
    #[account(init, payer = admin, token::mint = token_mint, token::authority = presale)]
    pub presale_wallet: Account<'info, TokenAccount>,
    #[account(mut)]
    pub merchant_wallet: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct SetStage<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        mut,
        has_one = admin,
        seeds = [PRESALE_SEED, admin.key().as_ref()],
        bump
    )]
    pub presale: Account<'info, Prescale>,
}

#[derive(Accounts)]
pub struct UpdateSalePeriod<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        mut,
        has_one = admin,
        seeds = [PRESALE_SEED, admin.key().as_ref()],
        bump
    )]
    pub presale: Account<'info, Presale>,
}

/// Accounts for the `buy_tokens` instruction (SOL payment).
#[derive(Accounts)]
pub struct BuyTokens<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>, 

    #[account(
        mut,
        seeds = [PRESALE_SEED, presale.admin.as_ref()],
        bump,
    )]
    pub presale: Account<'info, Presale>, 

    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, // Store presale tokens

    #[account(mut, address = presale.merchant_wallet)]
    /// CHECK: Checked by presale.merchant_wallet
    pub merchant_wallet: AccountInfo<'info>,

    pub token_mint: Account<'info, Mint>, 

    pub system_program: Program<'info, System>, // Required for SOL transfer
    pub token_program: Program<'info, Token>,   // Required for token transfers
    pub associated_token_program: Program<'info, AssociatedToken>, // Required for ATA creation
}

/// Accounts for the `check_presale_token_balance` instruction.
#[derive(Accounts)]
pub struct CheckPresaleTokenBalance<'info> {
    #[account(
        seeds = [PRESALE_SEED, presale.admin.as_ref()],
        bump,
    )]
    pub presale: Account<'info, Presale>, // Presale storage PDA

    #[account()]
    pub presale_wallet: Account<'info, TokenAccount>, 

    pub token_mint: Account<'info, Mint>, 
}

/// Accounts for the `update_sale_price` instruction.
#[derive(Accounts)]
pub struct UpdateSalePrice<'info> {
    #[account(mut)]
    pub admin: Signer<'info>, // Only  admin 
    #[account(
        mut,
        has_one = admin, // Ensures only  admin can update
        seeds = [PRESALE_SEED, admin.key().as_ref()],
        bump
    )]
    pub presale: Account<'info, Presale>,
}

/// Accounts for the `buy_tokens_by_stable_coin` instruction.
#[derive(Accounts)]
pub struct BuyTokensByStableCoin<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>, 

    #[account(
        mut,
        seeds = [PRESALE_SEED, presale.admin.as_ref()],
        bump,
    )]
    pub presale: Account<'info, Presale>, 

    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, // Presale token storage

    #[account(mut)]
    pub buyer_stable_coin_account: Account<'info, TokenAccount>, // Buyer’s stablecoin account

    #[account(mut)]
    pub merchant_stable_coin_account: Account<'info, TokenAccount>, // Merchant’s stablecoin account

    #[account()]
    pub stable_coin_mint: Account<'info, Mint>, // Stablecoin mint (USDC or USDT)

    pub token_mint: Account<'info, Mint>, // To get token decimals for calculations

    pub token_program: Program<'info, Token>, // Token Program
    pub associated_token_program: Program<'info, AssociatedToken>, // Required for ATA creation
    pub system_program: Program<'info, System>, // Required for ATA creation
}

/// Accounts for the `finalize_presale` instruction.
#[derive(Accounts)]
pub struct FinalizePresale<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        mut,
        has_one = admin,
        seeds = [PRESALE_SEED, admin.key().as_ref()],
        bump
    )]
    pub presale: Account<'info, Presale>,

    #[account(mut)]
    pub presale_wallet: Account<'info, TokenAccount>, 

    #[account(mut)]
    pub liquidity_wallet: Account<'info, TokenAccount>, 

    pub token_mint: Account<'info, Mint>, 

    pub token_program: Program<'info, Token>, 
}

/// Event emitted when tokens are purchased with SOL.
#[event]
pub struct BuyTokensEvent {
    pub buyer: Pubkey,
    pub tokens_purchased: u64,
    pub sol_spent: u64,
    pub sol_price_lamports_per_nlov: u64, 
    pub payment_type: u8,
}

/// Event emitted when the sale price is updated.
#[event]
pub struct UpdateSalePriceEvent {
    pub admin: Pubkey,
    pub new_usd_price_cents: u64, 
    pub new_sol_price_lamports: u64, 
    pub sale_stage: u8,
}

/// Event emitted when tokens are purchased with a stablecoin.
#[event]
pub struct BuyTokensByStableCoinEvent {
    pub buyer: Pubkey,
    pub tokens_purchased: u64,   
    pub stable_coin_amount: u64, 
    pub payment_type: u8,
}

/// Event emitted when the presale is finalized.
#[event]
pub struct FinalizePresaleEvent {
    pub admin: Pubkey,
    pub unsold_presale_tokens: u64,  codes for the presale program.
#[error_code]
pub enum PresaleError {
    #[msg("Invalid token account provided.")]
    InvalidTokenAccount,

    #[msg("Private sale period is not over yet.")]
    PrivateSaleNotOver,

    #[msg("Public sale period is not over yet.")]
    PublicSaleNotOver,

    #[msg("The presale has already ended.")]
    SaleAlreadyEnded,

    #[msg("Presale is not active.")]
    PresaleNotActive,

    #[msg("Presale is active now.")]
    PresaleActive,

    #[msg("Not enough tokens available for purchase.")]
    InsufficientTokens,

    #[msg("Insufficient SOL sent for purchase.")]
    InsufficientFunds,

    #[msg("Invalid stable token. Only USDC or USDT is accepted.")]
    InvalidStableToken,

    #[msg("Not enoentStableCoin,

    #[msg("Invalid payment type. Please choose 0 for Web3 or 1 for Web2.")]
    InvalidPaymentType,

    #[msg("Invalid price: Equivalent USD value must be at least $1.")]
    InvalidPrice,

    #[msg("Unauthorized: Only the presale admin can perform this action.")]
    Unauthorized,

    #[msg("The liquidity pool has already been created.")]
    LiquidityPoolAlreadyCreated,

    #[msg("No unsold tokens available for transfer.")]
    NoUnsoldTokens,

    #[msg("Hardcap for tokens has been reached.")] 
}
}
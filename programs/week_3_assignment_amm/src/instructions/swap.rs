use crate::{constants::*, error::*, state::*};
use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

#[derive(Accounts)]
#[instruction(is_x: bool)]
pub struct Swap<'info> {
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,

    #[account(mut)]
    pub user: Signer<'info>,

    pub mint_x: Account<'info, Mint>,
    pub mint_y: Account<'info, Mint>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [POOL_CONFIG_SEED, pool_config.seed.to_le_bytes().as_ref()],
        bump = pool_config.config_bump,
    )]
    pub pool_config: Account<'info, PoolConfig>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = pool_config,
    )]
    pub vault_x: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = pool_config,
    )]
    pub vault_y: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = user,
    )]
    pub user_x: Account<'info, TokenAccount>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = user,
    )]
    pub user_y: Account<'info, TokenAccount>,

    /// Protocol treasury token account to collect protocol fee share
    #[account(
        mut,
        constraint = treasury_token_account.owner == pool_config.treasury @ AmmError::InvalidTreasuryAccount,
        constraint = if is_x {
            treasury_token_account.mint == pool_config.mint_x
        } else {
            treasury_token_account.mint == pool_config.mint_y
        } @ AmmError::InvalidTreasuryAccount
    )]
    pub treasury_token_account: Account<'info, TokenAccount>,
}

impl<'info> Swap<'info> {
    pub fn swap(&mut self, is_x: bool, amount_in: u64, min_amount_out: u64) -> Result<()> {
        require!(!self.pool_config.locked, AmmError::PoolLocked);
        require!(amount_in > 0, AmmError::ZeroAmount);

        let (reserve_in, reserve_out) = if is_x {
            (self.vault_x.amount, self.vault_y.amount)
        } else {
            (self.vault_y.amount, self.vault_x.amount)
        };

        let (amount_out, treasury_fee, _lp_fee) = self.calculate_swap_amounts(
            amount_in,
            reserve_in,
            reserve_out,
            self.pool_config.fee,
            self.pool_config.treasury_fee,
        )?;

        require!(amount_out >= min_amount_out, AmmError::SlippageExceeded);

        let (user_in, vault_in, user_out, vault_out) = if is_x {
            (
                self.user_x.to_account_info(),
                self.vault_x.to_account_info(),
                self.user_y.to_account_info(),
                self.vault_y.to_account_info(),
            )
        } else {
            (
                self.user_y.to_account_info(),
                self.vault_y.to_account_info(),
                self.user_x.to_account_info(),
                self.vault_x.to_account_info(),
            )
        };

        if treasury_fee > 0 {
            token::transfer(
                CpiContext::new(
                    self.token_program.key(),
                    Transfer {
                        from: user_in.clone(),
                        to: self.treasury_token_account.to_account_info(),
                        authority: self.user.to_account_info(),
                    },
                ),
                treasury_fee,
            )?;
        }

        let vault_deposit_amount = amount_in
            .checked_sub(treasury_fee)
            .ok_or(AmmError::MathOverflow)?;

        token::transfer(
            CpiContext::new(
                self.token_program.key(),
                Transfer {
                    from: user_in,
                    to: vault_in,
                    authority: self.user.to_account_info(),
                },
            ),
            vault_deposit_amount,
        )?;

        let seed_bytes = self.pool_config.seed.to_le_bytes();
        let signer_seeds: &[&[&[u8]]] = &[&[
            POOL_CONFIG_SEED,
            seed_bytes.as_ref(),
            &[self.pool_config.config_bump],
        ]];

        token::transfer(
            CpiContext::new_with_signer(
                self.token_program.key(),
                Transfer {
                    from: vault_out,
                    to: user_out,
                    authority: self.pool_config.to_account_info(),
                },
                signer_seeds,
            ),
            amount_out,
        )?;

        Ok(())
    }

    pub fn calculate_swap_amounts(
        &self,
        amount_in: u64,
        reserve_in: u64,
        reserve_out: u64,
        fee_bps: u8,
        treasury_fee_bps: u8,
    ) -> Result<(u64, u64, u64)> {
        if reserve_in == 0 || reserve_out == 0 {
            return err!(AmmError::ZeroLiquidity);
        }

        let amount_in_u128 = amount_in as u128;

        // Total fee in basis points
        let total_fee = amount_in_u128
            .checked_mul(fee_bps as u128)
            .ok_or(AmmError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(AmmError::MathOverflow)?;

        // Protocol treasury fee
        let treasury_fee = amount_in_u128
            .checked_mul(treasury_fee_bps as u128)
            .ok_or(AmmError::MathOverflow)?
            .checked_div(BPS_DENOMINATOR)
            .ok_or(AmmError::MathOverflow)?;

        let lp_fee = total_fee
            .checked_sub(treasury_fee)
            .ok_or(AmmError::MathOverflow)?;

        let net_amount_in = amount_in_u128
            .checked_sub(total_fee)
            .ok_or(AmmError::MathOverflow)?;

        // Constant product formula: dy = (R_out * dx_net) / (R_in + dx_net)
        let numerator = (reserve_out as u128)
            .checked_mul(net_amount_in)
            .ok_or(AmmError::MathOverflow)?;
        let denominator = (reserve_in as u128)
            .checked_add(net_amount_in)
            .ok_or(AmmError::MathOverflow)?;

        let amount_out = numerator
            .checked_div(denominator)
            .ok_or(AmmError::MathOverflow)?;

        Ok((amount_out as u64, treasury_fee as u64, lp_fee as u64))
    }
}

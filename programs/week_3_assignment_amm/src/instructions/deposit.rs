use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount, Transfer},
};
use constant_product_curve::ConstantProduct;

use crate::{constants::*, error::*, state::PoolConfig};

#[derive(Accounts)]
pub struct Deposit<'info> {
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
        seeds = [LP_SEED, pool_config.key().as_ref()],
        bump = pool_config.lp_bump
    )]
    pub mint_lp: Account<'info, Mint>,

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

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = mint_lp,
        associated_token::authority = user,
    )]
    pub user_lp: Account<'info, TokenAccount>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, amount: u64, max_x: u64, max_y: u64) -> Result<()> {
        require!(!self.pool_config.locked, AmmError::PoolLocked);
        require!(amount > 0, AmmError::ZeroAmount);

        let (x, y) = match self.mint_lp.supply == 0 {
            true => (max_x, max_y),
            false => {
                let amounts = ConstantProduct::xy_deposit_amounts_from_l(
                    self.vault_x.amount,
                    self.vault_y.amount,
                    self.mint_lp.supply,
                    amount,
                    10u32.pow(self.mint_lp.decimals as u32),
                )
                .map_err(|_| AmmError::CurveError)?;

                require!(
                    amounts.x <= max_x && amounts.y <= max_y,
                    AmmError::SlippageExceeded
                );

                (amounts.x, amounts.y)
            }
        };

        // Transfer Token X from user to Vault X
        token::transfer(
            CpiContext::new(
                self.token_program.key(),
                Transfer {
                    from: self.user_x.to_account_info(),
                    to: self.vault_x.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            x,
        )?;

        // Transfer Token Y from user to Vault Y
        token::transfer(
            CpiContext::new(
                self.token_program.key(),
                Transfer {
                    from: self.user_y.to_account_info(),
                    to: self.vault_y.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            y,
        )?;

        // Mint LP tokens to user with config PDA signer seeds
        let seed_bytes = self.pool_config.seed.to_le_bytes();
        let signer_seeds: &[&[&[u8]]] = &[&[
            POOL_CONFIG_SEED,
            seed_bytes.as_ref(),
            &[self.pool_config.config_bump],
        ]];

        token::mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.mint_lp.to_account_info(),
                    to: self.user_lp.to_account_info(),
                    authority: self.pool_config.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;

        Ok(())
    }
}

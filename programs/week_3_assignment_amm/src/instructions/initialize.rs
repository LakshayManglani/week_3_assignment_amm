use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{constants::*, state::PoolConfig};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Initialize<'info> {
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,

    #[account(mut)]
    pub initializer: Signer<'info>,

    pub mint_x: Account<'info, Mint>,
    pub mint_y: Account<'info, Mint>,

    #[account(
        init,
        payer = initializer,
        space = 8 + PoolConfig::INIT_SPACE,
        seeds = [POOL_CONFIG_SEED, seed.to_le_bytes().as_ref()],
        bump,
    )]
    pub pool_config: Account<'info, PoolConfig>,

    #[account(
        init,
        payer = initializer,
        seeds = [LP_SEED, pool_config.key().as_ref()],
        bump,
        mint::decimals = 6,
        mint::authority = pool_config
    )]
    pub mint_lp: Account<'info, Mint>,

    #[account(
        init,
        payer = initializer,
        associated_token::mint = mint_x,
        associated_token::authority = pool_config,
    )]
    pub vault_x: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = initializer,
        associated_token::mint = mint_y,
        associated_token::authority = pool_config,
    )]
    pub vault_y: Account<'info, TokenAccount>,

    /// CHECK: Treasury authority
    pub treasury: UncheckedAccount<'info>,
}

impl<'info> Initialize<'info> {
    pub fn init(
        &mut self,
        seed: u64,
        fee: u8,
        treasury_fee: u8,
        authority: Option<Pubkey>,
        bumps: InitializeBumps,
    ) -> Result<()> {
        self.pool_config.set_inner(PoolConfig {
            seed,
            authority,
            treasury: self.treasury.key(),
            mint_x: self.mint_x.key(),
            mint_y: self.mint_y.key(),
            fee,
            treasury_fee,
            locked: false,
            config_bump: bumps.pool_config,
            lp_bump: bumps.mint_lp,
        });

        Ok(())
    }
}

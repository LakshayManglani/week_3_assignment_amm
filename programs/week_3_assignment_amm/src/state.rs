use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct PoolConfig {
    pub seed: u64,
    pub authority: Option<Pubkey>,

    pub mint_x: Pubkey,
    pub mint_y: Pubkey,
    pub treasury: Pubkey,

    pub fee: u8,
    pub treasury_fee: u8,

    pub locked: bool,

    pub config_bump: u8,
    pub lp_bump: u8,
}

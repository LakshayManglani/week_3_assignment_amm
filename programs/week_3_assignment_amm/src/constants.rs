use anchor_lang::prelude::*;

#[constant]
pub const POOL_CONFIG_SEED: &[u8] = b"pool_config";

#[constant]
pub const LP_SEED: &[u8] = b"lp";

#[constant]
pub const BPS_DENOMINATOR: u128 = 10_000;

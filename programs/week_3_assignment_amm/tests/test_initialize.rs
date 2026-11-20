use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{
            instruction::Instruction,
            program_option::COption,
            program_pack::Pack,
            system_program,
        },
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::get_associated_token_address,
        token::spl_token::state::{Account as TokenAccountState, AccountState, Mint as MintState},
    },
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    week_3_assignment_amm::{
        constants::{LP_SEED, POOL_CONFIG_SEED},
        state::PoolConfig,
    },
};

fn to_anchor_pubkey(kp: &Keypair) -> Pubkey {
    Pubkey::new_from_array(kp.pubkey().to_bytes())
}

fn to_svm_address(pubkey: &Pubkey) -> solana_pubkey::Pubkey {
    solana_pubkey::Pubkey::new_from_array(pubkey.to_bytes())
}

fn setup_svm() -> (LiteSVM, Pubkey) {
    let program_id = week_3_assignment_amm::id();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/week_3_assignment_amm.so"
    ));
    svm.add_program(to_svm_address(&program_id), bytes).unwrap();
    (svm, program_id)
}

fn create_mint_account(svm: &mut LiteSVM, mint: Pubkey, authority: Pubkey, decimals: u8) {
    let mint_data = MintState {
        mint_authority: COption::Some(authority),
        supply: 0,
        decimals,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    let mut data = vec![0u8; MintState::LEN];
    MintState::pack(mint_data, &mut data).unwrap();
    svm.set_account(
        to_svm_address(&mint),
        Account {
            lamports: 10_000_000,
            data,
            owner: to_svm_address(&anchor_spl::token::ID),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn create_token_account(
    svm: &mut LiteSVM,
    address: Pubkey,
    owner: Pubkey,
    mint: Pubkey,
    amount: u64,
) {
    let token_data = TokenAccountState {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    let mut data = vec![0u8; TokenAccountState::LEN];
    TokenAccountState::pack(token_data, &mut data).unwrap();
    svm.set_account(
        to_svm_address(&address),
        Account {
            lamports: 10_000_000,
            data,
            owner: to_svm_address(&anchor_spl::token::ID),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn get_token_balance(svm: &LiteSVM, address: &Pubkey) -> u64 {
    let raw = svm
        .get_account(&to_svm_address(address))
        .expect("token account not found");
    TokenAccountState::unpack(&raw.data).unwrap().amount
}

fn send_tx(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
) -> Result<(), litesvm::types::FailedTransactionMetadata> {
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    svm.send_transaction(tx).map(|_| ())
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Initialize Instruction Test Case
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn test_initialize() {
    let (mut svm, program_id) = setup_svm();
    let payer = Keypair::new();
    let payer_anchor = to_anchor_pubkey(&payer);
    svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

    let mint_x = to_anchor_pubkey(&Keypair::new());
    let mint_y = to_anchor_pubkey(&Keypair::new());
    create_mint_account(&mut svm, mint_x, payer_anchor, 6);
    create_mint_account(&mut svm, mint_y, payer_anchor, 6);

    let treasury = Keypair::new();
    let treasury_anchor = to_anchor_pubkey(&treasury);
    let seed: u64 = 42;
    let fee: u8 = 30; // 0.3%
    let treasury_fee: u8 = 10; // 0.1%

    let (pool_config, _) = Pubkey::find_program_address(
        &[POOL_CONFIG_SEED, &seed.to_le_bytes()],
        &program_id,
    );
    let (mint_lp, _) = Pubkey::find_program_address(
        &[LP_SEED, pool_config.as_ref()],
        &program_id,
    );
    let vault_x = get_associated_token_address(&pool_config, &mint_x);
    let vault_y = get_associated_token_address(&pool_config, &mint_y);

    let ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Initialize {
            seed,
            fee,
            treasury_fee,
            authority: Some(payer_anchor),
        }
        .data(),
        week_3_assignment_amm::accounts::Initialize {
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            initializer: payer_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            treasury: treasury_anchor,
        }
        .to_account_metas(None),
    );

    let res = send_tx(&mut svm, ix, &payer);
    assert!(res.is_ok(), "Initialize instruction failed");

    // Verify PoolConfig state
    let config_account = svm.get_account(&to_svm_address(&pool_config)).unwrap();
    let mut data: &[u8] = &config_account.data;
    let config_state = PoolConfig::try_deserialize(&mut data).unwrap();
    assert_eq!(config_state.seed, seed);
    assert_eq!(config_state.fee, fee);
    assert_eq!(config_state.treasury_fee, treasury_fee);
    assert_eq!(config_state.authority, Some(payer_anchor));
    assert_eq!(config_state.mint_x, mint_x);
    assert_eq!(config_state.mint_y, mint_y);
    assert_eq!(config_state.treasury, treasury_anchor);
    assert!(!config_state.locked);

    // Verify LP mint and vaults were created
    assert!(svm.get_account(&to_svm_address(&mint_lp)).is_some());
    assert!(svm.get_account(&to_svm_address(&vault_x)).is_some());
    assert!(svm.get_account(&to_svm_address(&vault_y)).is_some());
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Deposit Instruction Test Case
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn test_deposit() {
    let (mut svm, program_id) = setup_svm();
    let user = Keypair::new();
    let user_anchor = to_anchor_pubkey(&user);
    svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    let mint_x = to_anchor_pubkey(&Keypair::new());
    let mint_y = to_anchor_pubkey(&Keypair::new());
    create_mint_account(&mut svm, mint_x, user_anchor, 6);
    create_mint_account(&mut svm, mint_y, user_anchor, 6);

    let treasury = Keypair::new();
    let treasury_anchor = to_anchor_pubkey(&treasury);
    let seed: u64 = 100;
    let (pool_config, _) = Pubkey::find_program_address(
        &[POOL_CONFIG_SEED, &seed.to_le_bytes()],
        &program_id,
    );
    let (mint_lp, _) = Pubkey::find_program_address(
        &[LP_SEED, pool_config.as_ref()],
        &program_id,
    );
    let vault_x = get_associated_token_address(&pool_config, &mint_x);
    let vault_y = get_associated_token_address(&pool_config, &mint_y);

    // 1. Initialize pool
    let init_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Initialize {
            seed,
            fee: 30,
            treasury_fee: 10,
            authority: Some(user_anchor),
        }
        .data(),
        week_3_assignment_amm::accounts::Initialize {
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            initializer: user_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            treasury: treasury_anchor,
        }
        .to_account_metas(None),
    );
    send_tx(&mut svm, init_ix, &user).unwrap();

    // 2. Fund user token accounts
    let user_x = get_associated_token_address(&user_anchor, &mint_x);
    let user_y = get_associated_token_address(&user_anchor, &mint_y);
    let user_lp = get_associated_token_address(&user_anchor, &mint_lp);
    create_token_account(&mut svm, user_x, user_anchor, mint_x, 50_000_000);
    create_token_account(&mut svm, user_y, user_anchor, mint_y, 50_000_000);

    // 3. Deposit liquidity (initial deposit sets max_x and max_y)
    let deposit_amount = 10_000_000;
    let deposit_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Deposit {
            amount_x: deposit_amount,
            amount_y: deposit_amount,
            min_lp_out: deposit_amount,
        }
        .data(),
        week_3_assignment_amm::accounts::Deposit {
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
            user: user_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            user_x,
            user_y,
            user_lp,
        }
        .to_account_metas(None),
    );
    let res = send_tx(&mut svm, deposit_ix, &user);
    assert!(res.is_ok(), "Deposit instruction failed");

    // Verify user received LP tokens and vaults received deposited tokens
    assert_eq!(get_token_balance(&svm, &vault_x), deposit_amount);
    assert_eq!(get_token_balance(&svm, &vault_y), deposit_amount);
    assert_eq!(get_token_balance(&svm, &user_lp), deposit_amount);
    assert_eq!(get_token_balance(&svm, &user_x), 40_000_000);
    assert_eq!(get_token_balance(&svm, &user_y), 40_000_000);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Swap Instruction Test Case
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn test_swap() {
    let (mut svm, program_id) = setup_svm();
    let lp_provider = Keypair::new();
    let swapper = Keypair::new();
    let treasury = Keypair::new();
    let lp_anchor = to_anchor_pubkey(&lp_provider);
    let swapper_anchor = to_anchor_pubkey(&swapper);
    let treasury_anchor = to_anchor_pubkey(&treasury);

    svm.airdrop(&lp_provider.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&swapper.pubkey(), 10_000_000_000).unwrap();

    let mint_x = to_anchor_pubkey(&Keypair::new());
    let mint_y = to_anchor_pubkey(&Keypair::new());
    create_mint_account(&mut svm, mint_x, lp_anchor, 6);
    create_mint_account(&mut svm, mint_y, lp_anchor, 6);

    let seed: u64 = 200;
    let (pool_config, _) = Pubkey::find_program_address(
        &[POOL_CONFIG_SEED, &seed.to_le_bytes()],
        &program_id,
    );
    let (mint_lp, _) = Pubkey::find_program_address(
        &[LP_SEED, pool_config.as_ref()],
        &program_id,
    );
    let vault_x = get_associated_token_address(&pool_config, &mint_x);
    let vault_y = get_associated_token_address(&pool_config, &mint_y);

    // 1. Initialize pool
    let init_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Initialize {
            seed,
            fee: 30, // 30 bps = 0.3%
            treasury_fee: 10, // 10 bps = 0.1%
            authority: Some(lp_anchor),
        }
        .data(),
        week_3_assignment_amm::accounts::Initialize {
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            initializer: lp_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            treasury: treasury_anchor,
        }
        .to_account_metas(None),
    );
    send_tx(&mut svm, init_ix, &lp_provider).unwrap();

    // 2. Deposit initial liquidity (10_000_000 of each token)
    let lp_user_x = get_associated_token_address(&lp_anchor, &mint_x);
    let lp_user_y = get_associated_token_address(&lp_anchor, &mint_y);
    let lp_user_lp = get_associated_token_address(&lp_anchor, &mint_lp);
    create_token_account(&mut svm, lp_user_x, lp_anchor, mint_x, 20_000_000);
    create_token_account(&mut svm, lp_user_y, lp_anchor, mint_y, 20_000_000);

    let deposit_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Deposit {
            amount_x: 10_000_000,
            amount_y: 10_000_000,
            min_lp_out: 10_000_000,
        }
        .data(),
        week_3_assignment_amm::accounts::Deposit {
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
            user: lp_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            user_x: lp_user_x,
            user_y: lp_user_y,
            user_lp: lp_user_lp,
        }
        .to_account_metas(None),
    );
    send_tx(&mut svm, deposit_ix, &lp_provider).unwrap();

    // 3. Setup swapper accounts and treasury token account for token X
    let swapper_x = get_associated_token_address(&swapper_anchor, &mint_x);
    let swapper_y = get_associated_token_address(&swapper_anchor, &mint_y);
    let treasury_token_acc = get_associated_token_address(&treasury_anchor, &mint_x);
    create_token_account(&mut svm, swapper_x, swapper_anchor, mint_x, 1_000_000);
    create_token_account(&mut svm, swapper_y, swapper_anchor, mint_y, 0);
    create_token_account(&mut svm, treasury_token_acc, treasury_anchor, mint_x, 0);

    // 4. Swap 1_000_000 of Token X for Token Y
    let amount_in: u64 = 1_000_000;
    let min_amount_out: u64 = 1;
    let swap_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Swap {
            is_x: true,
            amount_in,
            min_amount_out,
        }
        .data(),
        week_3_assignment_amm::accounts::Swap {
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
            user: swapper_anchor,
            mint_x,
            mint_y,
            pool_config,
            vault_x,
            vault_y,
            user_x: swapper_x,
            user_y: swapper_y,
            treasury_token_account: treasury_token_acc,
        }
        .to_account_metas(None),
    );
    let res = send_tx(&mut svm, swap_ix, &swapper);
    assert!(res.is_ok(), "Swap instruction failed");

    // Verify swapper received Token Y and treasury received fee
    let swapper_y_balance = get_token_balance(&svm, &swapper_y);
    let treasury_fee_collected = get_token_balance(&svm, &treasury_token_acc);
    assert!(swapper_y_balance > 0, "Swapper should receive token Y");
    assert!(treasury_fee_collected > 0, "Treasury should collect fee");
    assert_eq!(get_token_balance(&svm, &swapper_x), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Withdraw Instruction Test Case
// ─────────────────────────────────────────────────────────────────────────────
#[test]
fn test_withdraw() {
    let (mut svm, program_id) = setup_svm();
    let user = Keypair::new();
    let treasury = Keypair::new();
    let user_anchor = to_anchor_pubkey(&user);
    let treasury_anchor = to_anchor_pubkey(&treasury);
    svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    let mint_x = to_anchor_pubkey(&Keypair::new());
    let mint_y = to_anchor_pubkey(&Keypair::new());
    create_mint_account(&mut svm, mint_x, user_anchor, 6);
    create_mint_account(&mut svm, mint_y, user_anchor, 6);

    let seed: u64 = 300;
    let (pool_config, _) = Pubkey::find_program_address(
        &[POOL_CONFIG_SEED, &seed.to_le_bytes()],
        &program_id,
    );
    let (mint_lp, _) = Pubkey::find_program_address(
        &[LP_SEED, pool_config.as_ref()],
        &program_id,
    );
    let vault_x = get_associated_token_address(&pool_config, &mint_x);
    let vault_y = get_associated_token_address(&pool_config, &mint_y);

    // 1. Initialize pool
    let init_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Initialize {
            seed,
            fee: 30,
            treasury_fee: 10,
            authority: Some(user_anchor),
        }
        .data(),
        week_3_assignment_amm::accounts::Initialize {
            system_program: system_program::ID,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            initializer: user_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            treasury: treasury_anchor,
        }
        .to_account_metas(None),
    );
    send_tx(&mut svm, init_ix, &user).unwrap();

    // 2. Fund user accounts & deposit initial liquidity (10_000_000 of each token)
    let user_x = get_associated_token_address(&user_anchor, &mint_x);
    let user_y = get_associated_token_address(&user_anchor, &mint_y);
    let user_lp = get_associated_token_address(&user_anchor, &mint_lp);
    create_token_account(&mut svm, user_x, user_anchor, mint_x, 10_000_000);
    create_token_account(&mut svm, user_y, user_anchor, mint_y, 10_000_000);

    let deposit_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Deposit {
            amount_x: 10_000_000,
            amount_y: 10_000_000,
            min_lp_out: 10_000_000,
        }
        .data(),
        week_3_assignment_amm::accounts::Deposit {
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
            user: user_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            user_x,
            user_y,
            user_lp,
        }
        .to_account_metas(None),
    );
    send_tx(&mut svm, deposit_ix, &user).unwrap();
    assert_eq!(get_token_balance(&svm, &user_lp), 10_000_000);

    // 3. Withdraw 50% of liquidity (burn 5_000_000 LP tokens)
    let withdraw_amount = 5_000_000;
    let withdraw_ix = Instruction::new_with_bytes(
        program_id,
        &week_3_assignment_amm::instruction::Withdraw {
            amount: withdraw_amount,
            min_x: 1,
            min_y: 1,
        }
        .data(),
        week_3_assignment_amm::accounts::Withdraw {
            user: user_anchor,
            mint_x,
            mint_y,
            pool_config,
            mint_lp,
            vault_x,
            vault_y,
            user_x,
            user_y,
            user_lp,
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    let res = send_tx(&mut svm, withdraw_ix, &user);
    assert!(res.is_ok(), "Withdraw instruction failed");

    // Verify user burned LP tokens and received tokens X and Y back
    assert_eq!(get_token_balance(&svm, &user_lp), 5_000_000);
    assert_eq!(get_token_balance(&svm, &user_x), 5_000_000);
    assert_eq!(get_token_balance(&svm, &user_y), 5_000_000);
    assert_eq!(get_token_balance(&svm, &vault_x), 5_000_000);
    assert_eq!(get_token_balance(&svm, &vault_y), 5_000_000);
}

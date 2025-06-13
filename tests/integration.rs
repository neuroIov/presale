#![cfg(feature = "test-bpf")]

use anchor_lang::{prelude::*, solana_program::system_program, InstructionFn};
use anchor_spl::token;
use solana_program_test::{self, ProgramTest, ProgramTestContext};
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
    system_instruction,
};
use std::time::{SystemTime, UNIX_EPOCH};

use presale::{
    constant::{USDC_ADDRESS, USDT_ADDRESS, PRESALE_SEED},
    program::Presale,
    BuyTokensEvent, BuyTokensByStableCoinEvent, FinalizePresaleEvent, UpdateSalePriceEvent,
    PresaleError,
}; // Import all necessary types and constants

// Helper function to create a token account
async fn create_token_account(
    context: &mut ProgramTestContext,
    mint_pubkey: &Pubkey,
    owner_pubkey: &Pubkey,
) -> Pubkey {
    let token_account_rent = context
        .banks_client
        .get_rent()
        .await
        .unwrap()
        .minimum_balance(token::TokenAccount::LEN);

    let create_account_ix = system_instruction::create_account(
        &context.payer.pubkey(),
        &context.payer.pubkey(), // Use payer as owner for simplicity in account creation for ATAs
        token_account_rent,
        token::TokenAccount::LEN as u64,
        &token::ID,
    );

    let create_ata_ix = anchor_spl::associated_token::create_associated_token_account(
        &context.payer.pubkey(), // Payer to create ATA
        owner_pubkey,            // Owner of the ATA
        mint_pubkey,             // Mint for the ATA
        &token::ID,              // Token program ID
    );

    let mut transaction = Transaction::new_with_payer(
        &[create_account_ix, create_ata_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer], context.last_blockhash);

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    anchor_spl::associated_token::get_associated_token_address(owner_pubkey, mint_pubkey)
}

// Helper function to mint tokens to an account
async fn mint_to(
    context: &mut ProgramTestContext,
    mint_pubkey: &Pubkey,
    destination_pubkey: &Pubkey,
    mint_authority_keypair: &Keypair,
    amount: u64,
) {
    let mint_to_ix = token::mint_to(
        &token::ID,
        mint_pubkey,
        destination_pubkey,
        &mint_authority_keypair.pubkey(),
        &[],
        amount,
    )
    .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[mint_to_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, mint_authority_keypair], context.last_blockhash);

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_initialize_presale() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    // Add necessary accounts and programs for testing
    program_test.prefer_bpf(false); // For faster testing

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new(); // Represents NLOV token mint

    // Mock token mint for NLOV
    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX, // Sufficient lamports
            token::Mint::LEN,
            &token::ID,
        ),
    );

    let mut context = program_test.start().await;

    // Create a mock token mint with authority
    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9, // NLOV token decimals
    )
    .unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Calculate PDA for presale account
    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    // Create Presale PDA wallet
    let (presale_wallet_ata, _) = Pubkey::find_program_address(
        &[
            admin.pubkey().as_ref(),
            token::ID.as_ref(),
            presale_token_mint.pubkey().as_ref(),
        ],
        &anchor_spl::associated_token::ID,
    );

    // Create merchant wallet
    let merchant_wallet = Keypair::new();

    let usd_price_cents_per_nlov = 3; // $0.03
    let sol_price_lamports_per_nlov = 182_000_000; 
    let private_sale_duration_days = 7;
    let public_sale_duration_days = 14;
    let hardcap_tokens = 1_000_000; // 1 million NLOV tokens

    let ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov,
            sol_price_lamports_per_nlov,
            private_sale_duration_days,
            public_sale_duration_days,
            hardcap_tokens,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);

    context
        .banks_client
        .process_transaction(transaction)
        .await
        .unwrap();

    // Fetch the presale account and verify its state
    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();

    assert_eq!(presale_account.admin, admin.pubkey());
    assert_eq!(presale_account.usd_price_cents_per_nlov, usd_price_cents_per_nlov);
    assert_eq!(presale_account.sol_price_lamports_per_nlov, sol_price_lamports_per_nlov);
    assert_eq!(presale_account.private_sale_duration, private_sale_duration_days * 86400);
    assert_eq!(presale_account.public_sale_duration, public_sale_duration_days * 86400);
    assert_eq!(presale_account.sale_stage, 0); // Not Started
    assert_eq!(presale_account.total_sold, 0);
    assert_eq!(presale_account.pool_created, false);
    assert_eq!(presale_account.hardcap_tokens, hardcap_tokens);
    assert_eq!(presale_account.presale_wallet, presale_wallet_ata);
    assert_eq!(presale_account.merchant_wallet, merchant_wallet.pubkey());
}

#[tokio::test]
async fn test_set_stage() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    let mut context = program_test.start().await;

    // Initialize mint
    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &admin.pubkey(), // Use admin as owner for presale wallet ATA in tests, easier for minting
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: 1_000_000,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Stage 0 -> 1 (Private Sale)
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account.sale_stage, 1); // Private Sale

    // Advance time beyond private sale duration for testing
    context.warp_to_slot(context.last_blockhash.slot + (7 * 24 * 60 * 60) / 2).await.unwrap(); // Warp past private sale duration

    // Stage 1 -> 2 (Public Sale)
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account.sale_stage, 2); // Public Sale

    // Advance time beyond public sale duration for testing
    context.warp_to_slot(context.last_blockhash.slot + (14 * 24 * 60 * 60) / 2).await.unwrap(); // Warp past public sale duration

    // Stage 2 -> 3 (Ended)
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account.sale_stage, 3); // Ended
}

#[tokio::test]
async fn test_buy_tokens_sol() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let buyer = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new(); // NLOV token
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    // Fund admin and buyer
    program_test.add_account(
        admin.pubkey(),
        solana_sdk::account::Account::new(1_000_000_000_000, 0, &system_program::ID),
    );
    program_test.add_account(
        buyer.pubkey(),
        solana_sdk::account::Account::new(100_000_000_000, 0, &system_program::ID),
    );

    let mut context = program_test.start().await;

    // Initialize NLOV mint
    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9, // NLOV token decimals
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda, // Presale PDA is the authority for its wallet
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let usd_price_cents = 3;
    let sol_price_lamports = 182_000_000; // 0.182 SOL
    let initial_hardcap = 1_000_000_000_000_000_000; // Large hardcap for this test
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: usd_price_cents,
            sol_price_lamports_per_nlov: sol_price_lamports,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: initial_hardcap,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Mint some NLOV tokens to the presale wallet
    mint_to(
        &mut context,
        &presale_token_mint.pubkey(),
        &presale_wallet_ata,
        &token_mint_authority,
        1_000_000 * 10u64.pow(9), // 1 million NLOV tokens (with 9 decimals)
    ).await;

    // Set stage to private sale
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Test successful purchase (Web3)
    let lamports_to_send = sol_price_lamports * 10; // Buy 10 NLOV tokens
    let expected_tokens_purchased_user_units = 10;
    let expected_tokens_purchased_raw = expected_tokens_purchased_user_units * 10u64.pow(9);

    let initial_merchant_sol_balance = context.banks_client.get_balance(merchant_wallet.pubkey()).await.unwrap();

    let buy_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokens {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            token_mint: presale_token_mint.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokens {
            payment_type: 0, // Web3
            lamports_sent: lamports_to_send,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);

    // Simulate and check for event
    let result = context.banks_client.process_transaction(transaction).await;
    assert!(result.is_ok());

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account.total_sold, expected_tokens_purchased_raw);

    let final_merchant_sol_balance = context.banks_client.get_balance(merchant_wallet.pubkey()).await.unwrap();
    assert_eq!(final_merchant_sol_balance, initial_merchant_sol_balance + lamports_to_send);

    // Test with Insufficient SOL (should fail)
    let insufficient_lamports = sol_price_lamports / 2; // Less than 1 NLOV equivalent
    let buy_ix_fail = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokens {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            token_mint: presale_token_mint.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokens {
            payment_type: 0,
            lamports_sent: insufficient_lamports,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_fail],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::InvalidPrice.to_string()));

    // Test Web2 purchase (no SOL transfer, but `total_sold` updates)
    let lamports_for_web2 = sol_price_lamports * 5; // Buy 5 NLOV tokens
    let expected_tokens_purchased_web2_raw = 5 * 10u64.pow(9);

    let buy_ix_web2 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokens {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            token_mint: presale_token_mint.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokens {
            payment_type: 1, // Web2
            lamports_sent: lamports_for_web2,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_web2],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(
        presale_account.total_sold,
        expected_tokens_purchased_raw + expected_tokens_purchased_web2_raw
    );
    // Merchant SOL balance should remain the same as before the Web2 transaction
    assert_eq!(final_merchant_sol_balance, context.banks_client.get_balance(merchant_wallet.pubkey()).await.unwrap());
}

#[tokio::test]
async fn test_buy_tokens_stable_coin() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let buyer = Keypair::new();
    let token_mint_authority = Keypair::new(); // NLOV mint authority
    let presale_token_mint = Keypair::new(); // NLOV token
    let stable_coin_mint_authority = Keypair::new(); // USDC/USDT mint authority
    let merchant_wallet = Keypair::new();

    // Mock token mint for NLOV
    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    // Mock USDC mint
    program_test.add_account(
        USDC_ADDRESS,
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    // Fund admin and buyer
    program_test.add_account(
        admin.pubkey(),
        solana_sdk::account::Account::new(1_000_000_000_000, 0, &system_program::ID),
    );
    program_test.add_account(
        buyer.pubkey(),
        solana_sdk::account::Account::new(100_000_000_000, 0, &system_program::ID),
    );

    let mut context = program_test.start().await;

    // Initialize NLOV mint
    let create_NLOV_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9, // NLOV token decimals
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_NLOV_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Initialize USDC mint
    let create_usdc_mint_ix = token::initialize_mint(
        &token::ID,
        &USDC_ADDRESS,
        &stable_coin_mint_authority.pubkey(),
        None,
        6, // USDC decimals
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_usdc_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer], context.last_blockhash); // No additional signers needed for a mock mint already in ProgramTest
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda,
        &presale_token_mint.pubkey(),
    );

    let buyer_usdc_ata = create_token_account(&mut context, &USDC_ADDRESS, &buyer.pubkey()).await;
    let merchant_usdc_ata = create_token_account(&mut context, &USDC_ADDRESS, &merchant_wallet.pubkey()).await;

    // Initialize presale contract
    let usd_price_cents = 3; // $0.03 per NLOV
    let sol_price_lamports = 182_000_000;
    let initial_hardcap = 1_000_000_000_000_000_000; // Large hardcap
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: usd_price_cents,
            sol_price_lamports_per_nlov: sol_price_lamports,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: initial_hardcap,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Mint NLOV to presale wallet
    mint_to(
        &mut context,
        &presale_token_mint.pubkey(),
        &presale_wallet_ata,
        &token_mint_authority,
        1_000_000 * 10u64.pow(9), // 1 million NLOV tokens
    ).await;

    // Mint USDC to buyer
    mint_to(
        &mut context,
        &USDC_ADDRESS,
        &buyer_usdc_ata,
        &stable_coin_mint_authority,
        1000 * 10u64.pow(6), // 1000 USDC (with 6 decimals)
    ).await;

    // Set stage to private sale
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Test successful purchase (Web3)
    let usdc_to_send_user_units = 3; // Buy 100 NLOV tokens (3 USDC / $0.03 per NLOV = 100 NLOV)
    let expected_tokens_purchased_user_units = 100;
    let expected_tokens_purchased_raw = expected_tokens_purchased_user_units * 10u64.pow(9);
    let usdc_to_send_raw = usdc_to_send_user_units * 10u64.pow(6); // 3 USDC = 3_000_000 raw units

    let initial_merchant_usdc_balance = context.banks_client.get_token_account(merchant_usdc_ata).await.unwrap().unwrap().amount;
    let initial_buyer_usdc_balance = context.banks_client.get_token_account(buyer_usdc_ata).await.unwrap().unwrap().amount;

    let buy_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokensByStableCoin {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            buyer_stable_coin_account: buyer_usdc_ata,
            merchant_stable_coin_account: merchant_usdc_ata,
            stable_coin_mint: USDC_ADDRESS,
            token_mint: presale_token_mint.pubkey(),
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokensByStableCoin {
            payment_type: 0, // Web3
            stable_coin_amount_user_units: usdc_to_send_user_units,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[buy_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);

    let result = context.banks_client.process_transaction(transaction).await;
    assert!(result.is_ok());

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account.total_sold, expected_tokens_purchased_raw);

    let final_merchant_usdc_balance = context.banks_client.get_token_account(merchant_usdc_ata).await.unwrap().unwrap().amount;
    let final_buyer_usdc_balance = context.banks_client.get_token_account(buyer_usdc_ata).await.unwrap().unwrap().amount;

    assert_eq!(final_merchant_usdc_balance, initial_merchant_usdc_balance + usdc_to_send_raw);
    assert_eq!(final_buyer_usdc_balance, initial_buyer_usdc_balance - usdc_to_send_raw);

    // Test with Insufficient stablecoin (should fail due to InvalidPrice or InsufficientStableCoin)
    let insufficient_usdc = 0; // Less than $1 USD equivalent
    let buy_ix_fail = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokensByStableCoin {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            buyer_stable_coin_account: buyer_usdc_ata,
            merchant_stable_coin_account: merchant_usdc_ata,
            stable_coin_mint: USDC_ADDRESS,
            token_mint: presale_token_mint.pubkey(),
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokensByStableCoin {
            payment_type: 0,
            stable_coin_amount_user_units: insufficient_usdc,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_fail],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::InvalidPrice.to_string()));

    // Test Web2 purchase (no stablecoin transfer, but `total_sold` updates)
    let usdc_for_web2 = 5; // Buy more tokens
    let expected_tokens_purchased_web2_raw = (5 * 100 / usd_price_cents) * 10u64.pow(9); // Calculate based on cents
    
    let buy_ix_web2 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokensByStableCoin {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            buyer_stable_coin_account: buyer_usdc_ata,
            merchant_stable_coin_account: merchant_usdc_ata,
            stable_coin_mint: USDC_ADDRESS,
            token_mint: presale_token_mint.pubkey(),
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokensByStableCoin {
            payment_type: 1, // Web2
            stable_coin_amount_user_units: usdc_for_web2,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_web2],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(
        presale_account.total_sold,
        expected_tokens_purchased_raw + expected_tokens_purchased_web2_raw
    );
    // Merchant USDC balance should remain the same as before the Web2 transaction
    assert_eq!(final_merchant_usdc_balance, context.banks_client.get_token_account(merchant_usdc_ata).await.unwrap().unwrap().amount);
}

#[tokio::test]
async fn test_update_sale_price() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    let mut context = program_test.start().await;

    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &admin.pubkey(),
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: 1_000_000,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Set stage to private sale
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Update price
    let new_usd_price = 5; // $0.05
    let new_sol_price = 200_000_000; // 0.2 SOL
    let update_price_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::UpdateSalePrice {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::UpdateSalePrice {
            new_usd_price_cents: new_usd_price,
            new_sol_price_lamports: new_sol_price,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[update_price_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();

    assert_eq!(presale_account.usd_price_cents_per_nlov, new_usd_price);
    assert_eq!(presale_account.sol_price_lamports_per_nlov, new_sol_price);
}

#[tokio::test]
async fn test_update_sale_period() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    let mut context = program_test.start().await;

    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &admin.pubkey(),
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: 1_000_000,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Update sale period
    let new_private_duration = 10;
    let new_public_duration = 20;
    let update_period_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::UpdateSalePeriod {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::UpdateSalePeriod {
            new_private_sale_duration_days: new_private_duration,
            new_public_sale_duration_days: new_public_duration,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[update_period_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();

    assert_eq!(presale_account.private_sale_duration, new_private_duration * 86400);
    assert_eq!(presale_account.public_sale_duration, new_public_duration * 86400);
}

#[tokio::test]
async fn test_check_presale_token_balance() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    let mut context = program_test.start().await;

    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda, // Presale PDA is the authority for its wallet
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let hardcap_tokens = 1_000_000;
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: hardcap_tokens,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Mint tokens to presale wallet
    let initial_presale_tokens = 500_000 * 10u64.pow(9);
    mint_to(
        &mut context,
        &presale_token_mint.pubkey(),
        &presale_wallet_ata,
        &token_mint_authority,
        initial_presale_tokens,
    ).await;

    // Check initial balance
    let check_balance_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::CheckPresaleTokenBalance {
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            token_mint: presale_token_mint.pubkey(),
        }
        .to_account_metas(None),
        data: presale::instruction::CheckPresaleTokenBalance {}.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[check_balance_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer], context.last_blockhash);
    let result = context.banks_client.process_transaction(transaction).await;
    assert!(result.is_ok());

    // You would typically assert the return value of `check_presale_token_balance` here,
    // but fetching directly from `get_account_data_with_borsh` is more straightforward in tests.
    let presale_wallet_account = context.banks_client.get_token_account(presale_wallet_ata).await.unwrap().unwrap();
    let presale_account: presale::Presale = context.banks_client.get_account_data_with_borsh(presale_pda).await.unwrap();

    assert_eq!(presale_wallet_account.amount.saturating_sub(presale_account.total_sold), initial_presale_tokens);
}

#[tokio::test]
async fn test_finalize_presale() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();
    let liquidity_wallet_owner = Keypair::new(); // Owner of the liquidity wallet

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    // Fund admin
    program_test.add_account(
        admin.pubkey(),
        solana_sdk::account::Account::new(1_000_000_000_000, 0, &system_program::ID),
    );

    let mut context = program_test.start().await;

    // Initialize NLOV mint
    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda,
        &presale_token_mint.pubkey(),
    );
    let liquidity_wallet_ata = create_token_account(&mut context, &presale_token_mint.pubkey(), &liquidity_wallet_owner.pubkey()).await;


    // Initialize presale contract with some tokens in presale_wallet
    let hardcap_tokens = 1_000_000 * 10u64.pow(9); // 1 million NLOV raw
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: hardcap_tokens,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Mint tokens to presale wallet
    let initial_presale_wallet_amount = 500_000 * 10u64.pow(9); // 500k NLOV
    mint_to(
        &mut context,
        &presale_token_mint.pubkey(),
        &presale_wallet_ata,
        &token_mint_authority,
        initial_presale_wallet_amount,
    ).await;

    // Simulate some tokens being sold
    let sold_tokens_raw = 100_000 * 10u64.pow(9); // 100k NLOV sold
    let mut presale_account: presale::Presale = context.banks_client.get_account_data_with_borsh(presale_pda).await.unwrap();
    presale_account.total_sold = sold_tokens_raw; // Manually set for testing finalization logic
    context.set_account_data(presale_pda, &presale_account.try_to_vec().unwrap());

    // Advance time to beyond public sale duration to end the sale
    context.warp_to_slot(context.last_blockhash.slot + (21 * 24 * 60 * 60) / 2).await.unwrap();

    // Set stage to ended
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Finalize presale
    let finalize_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::FinalizePresale {
            admin: admin.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            liquidity_wallet: liquidity_wallet_ata,
            token_mint: presale_token_mint.pubkey(),
            token_program: token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::FinalizePresale {}.data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[finalize_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);

    let result = context.banks_client.process_transaction(transaction).await;
    assert!(result.is_ok());

    let final_presale_account: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert!(final_presale_account.pool_created);

    let final_presale_wallet_balance = context.banks_client.get_token_account(presale_wallet_ata).await.unwrap().unwrap().amount;
    let final_liquidity_wallet_balance = context.banks_client.get_token_account(liquidity_wallet_ata).await.unwrap().unwrap().amount;

    let unsold_tokens = initial_presale_wallet_amount.checked_sub(sold_tokens_raw).unwrap();
    assert_eq!(final_presale_wallet_balance, 0); // All unsold tokens should be moved
    assert_eq!(final_liquidity_wallet_balance, unsold_tokens);
}

// Test for Hardcap Reached error
#[tokio::test]
async fn test_hardcap_reached() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let buyer = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    program_test.add_account(
        admin.pubkey(),
        solana_sdk::account::Account::new(1_000_000_000_000, 0, &system_program::ID),
    );
    program_test.add_account(
        buyer.pubkey(),
        solana_sdk::account::Account::new(100_000_000_000, 0, &system_program::ID),
    );

    let mut context = program_test.start().await;

    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda,
        &presale_token_mint.pubkey(),
    );

    // Set a very small hardcap for testing purposes
    let hardcap_tokens_user_units = 100; // Hardcap at 100 NLOV
    let hardcap_tokens_raw = hardcap_tokens_user_units * 10u64.pow(9);

    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: hardcap_tokens_raw,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    mint_to(
        &mut context,
        &presale_token_mint.pubkey(),
        &presale_wallet_ata,
        &token_mint_authority,
        hardcap_tokens_raw, // Mint exactly the hardcap amount
    ).await;

    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Purchase tokens up to the hardcap
    let buy_amount_user_units_1 = 50;
    let lamports_to_send_1 = 182_000_000 * buy_amount_user_units_1;
    let buy_ix_1 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokens {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            token_mint: presale_token_mint.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokens {
            payment_type: 0,
            lamports_sent: lamports_to_send_1,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_1],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let presale_account_after_first_buy: presale::Presale = context
        .banks_client
        .get_account_data_with_borsh(presale_pda)
        .await
        .unwrap();
    assert_eq!(presale_account_after_first_buy.total_sold, buy_amount_user_units_1 * 10u64.pow(9));

    // Attempt to buy more tokens, exceeding hardcap
    let buy_amount_user_units_2 = 60; // 50 + 60 = 110, which is > 100 hardcap
    let lamports_to_send_2 = 182_000_000 * buy_amount_user_units_2;
    let buy_ix_2 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::BuyTokens {
            buyer: buyer.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            token_mint: presale_token_mint.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::BuyTokens {
            payment_type: 0,
            lamports_sent: lamports_to_send_2,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[buy_ix_2],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &buyer], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::HardcapReached.to_string()));
}

#[tokio::test]
async fn test_unauthorized_actions() {
    let mut program_test = ProgramTest::new(
        "presale",
        presale::id(),
        None,
    );

    let admin = Keypair::new();
    let unauthorized_user = Keypair::new();
    let token_mint_authority = Keypair::new();
    let presale_token_mint = Keypair::new();
    let merchant_wallet = Keypair::new();

    program_test.add_account(
        presale_token_mint.pubkey(),
        solana_sdk::account::Account::new(
            u64::MAX,
            token::Mint::LEN,
            &token::ID,
        ),
    );

    program_test.add_account(
        admin.pubkey(),
        solana_sdk::account::Account::new(1_000_000_000_000, 0, &system_program::ID),
    );
    program_test.add_account(
        unauthorized_user.pubkey(),
        solana_sdk::account::Account::new(100_000_000_000, 0, &system_program::ID),
    );

    let mut context = program_test.start().await;

    let create_mint_ix = token::initialize_mint(
        &token::ID,
        &presale_token_mint.pubkey(),
        &token_mint_authority.pubkey(),
        None,
        9,
    )
    .unwrap();
    let mut transaction = Transaction::new_with_payer(
        &[create_mint_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &presale_token_mint], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    let (presale_pda, _presale_bump) = Pubkey::find_program_address(
        &[PRESALE_SEED, admin.pubkey().as_ref()],
        &presale::id(),
    );

    let presale_wallet_ata = anchor_spl::associated_token::get_associated_token_address(
        &presale_pda,
        &presale_token_mint.pubkey(),
    );

    // Initialize presale contract
    let init_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::Initialize {
            admin: admin.pubkey(),
            presale: presale_pda,
            token_mint: presale_token_mint.pubkey(),
            presale_wallet: presale_wallet_ata,
            merchant_wallet: merchant_wallet.pubkey(),
            system_program: system_program::ID,
            token_program: token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::Initialize {
            usd_price_cents_per_nlov: 3,
            sol_price_lamports_per_nlov: 182_000_000,
            private_sale_duration_days: 7,
            public_sale_duration_days: 14,
            hardcap_tokens: 1_000_000,
        }
        .data(),
    };

    let mut transaction = Transaction::new_with_payer(
        &[init_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap();

    // Test unauthorized `set_stage`
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: unauthorized_user.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[set_stage_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &unauthorized_user], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::Unauthorized.to_string()));

    // Test unauthorized `update_sale_price`
    let update_price_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::UpdateSalePrice {
            admin: unauthorized_user.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::UpdateSalePrice {
            new_usd_price_cents: 10,
            new_sol_price_lamports: 300_000_000,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[update_price_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &unauthorized_user], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::Unauthorized.to_string()));

    // Test unauthorized `update_sale_period`
    let update_period_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::UpdateSalePeriod {
            admin: unauthorized_user.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::UpdateSalePeriod {
            new_private_sale_duration_days: 1,
            new_public_sale_duration_days: 1,
        }
        .data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[update_period_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &unauthorized_user], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::Unauthorized.to_string()));

    // Test unauthorized `finalize_presale`
    let liquidity_wallet_owner = Keypair::new();
    let liquidity_wallet_ata = create_token_account(&mut context, &presale_token_mint.pubkey(), &liquidity_wallet_owner.pubkey()).await;

    // First, set stage to 3 (Ended) to make it valid for finalization
    let set_stage_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(&[set_stage_ix], Some(&context.payer.pubkey()));
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap(); // Stage 0 -> 1
    
    context.warp_to_slot(context.last_blockhash.slot + (21 * 24 * 60 * 60) / 2).await.unwrap(); // Advance time
    
    let set_stage_ix_2 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(&[set_stage_ix_2], Some(&context.payer.pubkey()));
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap(); // Stage 1 -> 2
    
    context.warp_to_slot(context.last_blockhash.slot + (21 * 24 * 60 * 60) / 2).await.unwrap(); // Advance time
    
    let set_stage_ix_3 = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::SetStage {
            admin: admin.pubkey(),
            presale: presale_pda,
        }
        .to_account_metas(None),
        data: presale::instruction::SetStage {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(&[set_stage_ix_3], Some(&context.payer.pubkey()));
    transaction.sign(&[&context.payer, &admin], context.last_blockhash);
    context.banks_client.process_transaction(transaction).await.unwrap(); // Stage 2 -> 3

    let finalize_ix = Instruction {
        program_id: presale::id(),
        accounts: presale::accounts::FinalizePresale {
            admin: unauthorized_user.pubkey(),
            presale: presale_pda,
            presale_wallet: presale_wallet_ata,
            liquidity_wallet: liquidity_wallet_ata,
            token_mint: presale_token_mint.pubkey(),
            token_program: token::ID,
        }
        .to_account_metas(None),
        data: presale::instruction::FinalizePresale {}.data(),
    };
    let mut transaction = Transaction::new_with_payer(
        &[finalize_ix],
        Some(&context.payer.pubkey()),
    );
    transaction.sign(&[&context.payer, &unauthorized_user], context.last_blockhash);
    let err = context.banks_client.process_transaction(transaction).await.unwrap_err();
    assert!(err.to_string().contains(&PresaleError::Unauthorized.to_string()));
}
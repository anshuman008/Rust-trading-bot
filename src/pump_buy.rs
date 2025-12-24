use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
#[allow(deprecated)]
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
    transaction::Transaction,
};
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account,
};
use spl_token::ID as TOKEN_PROGRAM_ID;
use spl_token_2022::ID as TOKEN_2022_PROGRAM_ID;
use std::str::FromStr;
use crate::cal;


// Constants
const MAINNET_RPC: &str = "https://api.mainnet-beta.solana.com";
const PRIVATE_KEY: &str = "priv-key";
const FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";

lazy_static::lazy_static! {
    static ref PUMP_PROGRAM_ID: Pubkey = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap();
    static ref GLOBAL_ADDRESS: Pubkey = Pubkey::from_str("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf").unwrap();
    static ref EVENT_AUTHORITY: Pubkey = Pubkey::from_str("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1").unwrap();
    static ref FEE_PROGRAM: Pubkey = Pubkey::from_str("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ").unwrap();
    static ref FEE_CONFIG: Pubkey = Pubkey::from_str("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt").unwrap();
}

/// Buy instruction discriminator
const BUY_DISCRIMINATOR: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];

/// Accounts needed for the buy instruction
pub struct BuyAccounts {
    pub global: Pubkey,
    pub fee_recipient: Pubkey,
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub associated_user: Pubkey,
    pub user: Pubkey,
    pub system_program: Pubkey,
    pub token_program: Pubkey,
    pub creator_vault: Pubkey,
    pub event_authority: Pubkey,
    pub program: Pubkey,
    pub global_volume_accumulator: Pubkey,
    pub user_volume_accumulator: Pubkey,
    pub fee_config: Pubkey,
    pub fee_program: Pubkey,
}

/// Arguments for the buy instruction
pub struct BuyArgs {
    pub amount: u64,
    pub max_sol_cost: u64,
    pub track_volume: bool,
}

/// Load wallet from base58 encoded private key
fn load_wallet_from_private_key(private_key: &str) -> Result<Keypair> {
    let secret_key = bs58::decode(private_key)
        .into_vec()
        .map_err(|e| anyhow!("Failed to decode private key: {}", e))?;
    Keypair::try_from(secret_key.as_slice()).map_err(|e| anyhow!("Failed to create keypair: {}", e))
}

/// Create the buy instruction
fn create_buy_instruction(accounts: BuyAccounts, args: BuyArgs) -> Instruction {
    // Build instruction data: discriminator (8) + amount (8) + max_sol_cost (8) + Option<bool> (2)
    let mut data = Vec::with_capacity(26);

    // Add discriminator
    data.extend_from_slice(&BUY_DISCRIMINATOR);

    // Add amount (u64 little-endian)
    data.extend_from_slice(&args.amount.to_le_bytes());

    // Add max_sol_cost (u64 little-endian)
    data.extend_from_slice(&args.max_sol_cost.to_le_bytes());

    // Add track_volume as Option<bool>: Some = 1, then value
    data.push(1); // Some
    data.push(if args.track_volume { 1 } else { 0 });

    // Build account metas
    let keys = vec![
        AccountMeta::new_readonly(accounts.global, false),
        AccountMeta::new(accounts.fee_recipient, false),
        AccountMeta::new_readonly(accounts.mint, false),
        AccountMeta::new(accounts.bonding_curve, false),
        AccountMeta::new(accounts.associated_bonding_curve, false),
        AccountMeta::new(accounts.associated_user, false),
        AccountMeta::new(accounts.user, true),
        AccountMeta::new_readonly(accounts.system_program, false),
        AccountMeta::new_readonly(accounts.token_program, false),
        AccountMeta::new(accounts.creator_vault, false),
        AccountMeta::new_readonly(accounts.event_authority, false),
        AccountMeta::new_readonly(accounts.program, false),
        AccountMeta::new(accounts.global_volume_accumulator, false),
        AccountMeta::new(accounts.user_volume_accumulator, false),
        AccountMeta::new_readonly(accounts.fee_config, false),
        AccountMeta::new_readonly(accounts.fee_program, false),
    ];

    Instruction {
        program_id: *PUMP_PROGRAM_ID,
        accounts: keys,
        data,
    }
}

/// Derive the bonding curve PDA
fn get_bonding_curve_pda(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMP_PROGRAM_ID)
}

/// Derive the creator vault PDA
fn get_creator_vault_pda(creator: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"creator-vault", creator.as_ref()], &PUMP_PROGRAM_ID)
}

/// Derive the global volume accumulator PDA
fn get_global_volume_accumulator_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"global_volume_accumulator"], &PUMP_PROGRAM_ID)
}

/// Derive the user volume accumulator PDA
fn get_user_volume_accumulator_pda(user: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"user_volume_accumulator", user.as_ref()], &PUMP_PROGRAM_ID)
}

/// Parse creator pubkey from bonding curve account data
/// Layout: 8 (discriminator) + 8*5 (u64 fields) + 1 (bool) = 49 bytes, then 32 bytes for creator
fn parse_creator_from_bonding_curve(data: &[u8]) -> Result<Pubkey> {
    const CREATOR_OFFSET: usize = 8 + 8 + 8 + 8 + 8 + 8 + 1; // 49 bytes

    if data.len() < CREATOR_OFFSET + 32 {
        return Err(anyhow!(
            "Bonding curve data too short: {} bytes",
            data.len()
        ));
    }

    let creator_bytes: [u8; 32] = data[CREATOR_OFFSET..CREATOR_OFFSET + 32]
        .try_into()
        .map_err(|_| anyhow!("Failed to parse creator bytes"))?;

    Ok(Pubkey::new_from_array(creator_bytes))
}

/// Main function to execute the pump.fun buy
pub fn run_pump_buy(token_amount: u64,mint: Pubkey, max_sol_cost: u64) -> Result<()> {

   

    println!("Starting mainnet buy test...");
    println!("Token mint: {}", mint);

    // Initialize RPC client
    let connection = RpcClient::new(MAINNET_RPC.to_string());

    // Load wallet
    println!("Loading wallet from private key...");
    let user = load_wallet_from_private_key(PRIVATE_KEY)?;
    println!("User address: {}", user.pubkey());

    // Check balance
    let balance = connection.get_balance(&user.pubkey())?;
    let balance_sol = balance as f64 / LAMPORTS_PER_SOL as f64;
    println!("Wallet balance: {} SOL", balance_sol);

    if balance < max_sol_cost + 10_000_000 {
        return Err(anyhow!(
            "Insufficient balance. Need at least {} SOL",
            (max_sol_cost + 10_000_000) as f64 / LAMPORTS_PER_SOL as f64
        ));
    }

    // Parse addresses
    let fee_recipient = Pubkey::from_str(FEE_RECIPIENT)?;

    // Derive bonding curve PDA
    let (bonding_curve, _) = get_bonding_curve_pda(&mint);
    println!("Bonding Curve: {}", bonding_curve);

    // Get mint info to determine token program
    let mint_info = connection
        .get_account(&mint)
        .map_err(|e| anyhow!("Failed to get mint account: {}", e))?;

    let token_program_id = if mint_info.owner == TOKEN_2022_PROGRAM_ID {
        TOKEN_2022_PROGRAM_ID
    } else {
        TOKEN_PROGRAM_ID
    };
    println!("Token Program: {}", token_program_id);

    // Get associated token addresses
    let associated_bonding_curve =
        get_associated_token_address_with_program_id(&bonding_curve, &mint, &token_program_id);
    println!("Associated Bonding Curve: {}", associated_bonding_curve);

    let associated_user =
        get_associated_token_address_with_program_id(&user.pubkey(), &mint, &token_program_id);
    println!("Associated Token Account: {}", associated_user);

    // Fetch bonding curve to get creator
    let bonding_curve_info = connection
        .get_account(&bonding_curve)
        .map_err(|_| anyhow!("Bonding curve account not found - token may have migrated"))?;

    let creator = parse_creator_from_bonding_curve(&bonding_curve_info.data)?;
    println!("Token Creator: {}", creator);

    // Derive creator vault PDA
    let (creator_vault, _) = get_creator_vault_pda(&creator);
    println!("Creator Vault: {}", creator_vault);

    // Derive volume accumulator PDAs
    let (global_volume_accumulator, _) = get_global_volume_accumulator_pda();
    println!("Global Volume Accumulator: {}", global_volume_accumulator);

    let (user_volume_accumulator, _) = get_user_volume_accumulator_pda(&user.pubkey());
    println!("User Volume Accumulator: {}", user_volume_accumulator);

    println!("\nBuilding buy instruction...");
    println!("  Amount: {} tokens", token_amount);
    println!(
        "  Max SOL cost: {} SOL",
        max_sol_cost as f64 / LAMPORTS_PER_SOL as f64
    );

    // Create buy instruction
    let buy_ix = create_buy_instruction(
        BuyAccounts {
            global: *GLOBAL_ADDRESS,
            fee_recipient,
            mint,
            bonding_curve,
            associated_bonding_curve,
            associated_user,
            user: user.pubkey(),
            system_program: system_program::ID,
            token_program: token_program_id,
            creator_vault,
            event_authority: *EVENT_AUTHORITY,
            program: *PUMP_PROGRAM_ID,
            global_volume_accumulator,
            user_volume_accumulator,
            fee_config: *FEE_CONFIG,
            fee_program: *FEE_PROGRAM,
        },
        BuyArgs {
            amount: token_amount,
            max_sol_cost: max_sol_cost,
            track_volume: true,
        },
    );

    // Get latest blockhash
    let blockhash = connection.get_latest_blockhash()?;

    // Build transaction
    let mut instructions = Vec::new();

    // Check if ATA exists, if not, create it
    if connection.get_account(&associated_user).is_err() {
        println!("Creating associated token account for user...");
        let create_ata_ix = create_associated_token_account(
            &user.pubkey(),   // payer
            &user.pubkey(),   // wallet
            &mint,            // mint
            &token_program_id // token program
        );
        instructions.push(create_ata_ix);
    }

    instructions.push(buy_ix);

    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&user.pubkey()),
        &[&user],
        blockhash,
    );

    // Simulate transaction
    println!("\nSimulating transaction...");
    
    // match connection.send_transaction(&transaction) {
    //     Ok(signature) => {
    //         println!("Transaction sent: {}", signature);
    //     }
    //     Err(e) => {
    //         println!("Failed to send transaction: {}", e);
    //     }
    // }
        
    

    match connection.simulate_transaction(&transaction) {
        Ok(simulation) => {
            println!("Simulation result:");
            println!("  Error: {:?}", simulation.value.err);
            println!("  Logs:");
            if let Some(logs) = &simulation.value.logs {
                for log in logs {
                    println!("    {}", log);
                }
            }
            println!("  Units consumed: {:?}", simulation.value.units_consumed);

            if simulation.value.err.is_none() {
                println!("\n✓ Simulation successful! Ready to send transaction.");

                // Uncomment below to actually send the transaction:
                // println!("\nSending transaction...");
                // let signature = connection.send_and_confirm_transaction(&transaction)?;
                // println!("✓ Buy successful!");
                // println!("Signature: {}", signature);
                // println!("View on Solscan: https://solscan.io/tx/{}", signature);
            }
        }
        Err(e) => {
            println!("✗ Failed to simulate transaction: {}", e);
        }
    }

    Ok(())
}


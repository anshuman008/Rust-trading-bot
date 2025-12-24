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
use spl_associated_token_account::get_associated_token_address_with_program_id;
use spl_token::ID as TOKEN_PROGRAM_ID;
use spl_token_2022::ID as TOKEN_2022_PROGRAM_ID;
use std::str::FromStr;

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

/// Sell instruction discriminator (from IDL: [51, 230, 133, 164, 1, 127, 131, 173])
const SELL_DISCRIMINATOR: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];

/// Accounts needed for the sell instruction
pub struct SellAccounts {
    pub global: Pubkey,
    pub fee_recipient: Pubkey,
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub associated_user: Pubkey,
    pub user: Pubkey,
    pub system_program: Pubkey,
    pub creator_vault: Pubkey,
    pub token_program: Pubkey,
    pub event_authority: Pubkey,
    pub program: Pubkey,
    pub fee_config: Pubkey,
    pub fee_program: Pubkey,
}

/// Arguments for the sell instruction
pub struct SellArgs {
    pub amount: u64,
    pub min_sol_output: u64,
}

/// Load wallet from base58 encoded private key
fn load_wallet_from_private_key(private_key: &str) -> Result<Keypair> {
    let secret_key = bs58::decode(private_key)
        .into_vec()
        .map_err(|e| anyhow!("Failed to decode private key: {}", e))?;
    Keypair::try_from(secret_key.as_slice()).map_err(|e| anyhow!("Failed to create keypair: {}", e))
}

/// Create the sell instruction
fn create_sell_instruction(accounts: SellAccounts, args: SellArgs) -> Instruction {
    // Build instruction data: discriminator (8) + amount (8) + min_sol_output (8)
    let mut data = Vec::with_capacity(24);

    // Add discriminator
    data.extend_from_slice(&SELL_DISCRIMINATOR);

    // Add amount (u64 little-endian)
    data.extend_from_slice(&args.amount.to_le_bytes());

    // Add min_sol_output (u64 little-endian)
    data.extend_from_slice(&args.min_sol_output.to_le_bytes());

    // Build account metas (order from IDL)
    let keys = vec![
        AccountMeta::new_readonly(accounts.global, false),
        AccountMeta::new(accounts.fee_recipient, false),
        AccountMeta::new_readonly(accounts.mint, false),
        AccountMeta::new(accounts.bonding_curve, false),
        AccountMeta::new(accounts.associated_bonding_curve, false),
        AccountMeta::new(accounts.associated_user, false),
        AccountMeta::new(accounts.user, true),
        AccountMeta::new_readonly(accounts.system_program, false),
        AccountMeta::new(accounts.creator_vault, false),
        AccountMeta::new_readonly(accounts.token_program, false),
        AccountMeta::new_readonly(accounts.event_authority, false),
        AccountMeta::new_readonly(accounts.program, false),
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

/// Main function to execute the pump.fun sell
pub fn run_pump_sell() -> Result<()> {


    let mint = Pubkey::from_str("Ar4vi1BZXHVgQFRYD8AF7rBe7gsh3D1nM2hZG153pump").unwrap();
    let min_sol_output: u64 = 0; // Minimum SOL to receive (slippage protection)
    let mut token_amount: u64 = 1000;
    println!("Starting mainnet sell test...");
    println!("Token mint: {}", mint);

    // Initialize RPC client
    let connection = RpcClient::new(MAINNET_RPC.to_string());

    // Load wallet
    println!("Loading wallet from private key...");
    let user = load_wallet_from_private_key(PRIVATE_KEY)?;
    println!("User address: {}", user.pubkey());

    // Check SOL balance
    let balance = connection.get_balance(&user.pubkey())?;
    let balance_sol = balance as f64 / LAMPORTS_PER_SOL as f64;
    println!("Wallet SOL balance: {} SOL", balance_sol);

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

    // Check if user has tokens to sell
    match connection.get_account(&associated_user) {
        Ok(ata_info) => {
            // Parse token balance (offset 64 for amount in token account)
            if ata_info.data.len() >= 72 {
                let amount_bytes: [u8; 8] = ata_info.data[64..72].try_into().unwrap();
                let token_balance = u64::from_le_bytes(amount_bytes);
                println!("Token balance: {}", token_balance);

                if token_balance == 0 {
                    return Err(anyhow!("No tokens to sell"));
                }

                if token_balance < token_amount {
                    return Err(anyhow!(
                        "Insufficient token balance. Have {} but trying to sell {}",
                        token_balance,
                        token_amount
                    ));
                }
                token_amount = token_balance
            }
        }
        Err(_) => {
            return Err(anyhow!("Token account not found - no tokens to sell"));
        }
    }

    // Fetch bonding curve to get creator
    let bonding_curve_info = connection
        .get_account(&bonding_curve)
        .map_err(|_| anyhow!("Bonding curve account not found - token may have migrated"))?;

    let creator = parse_creator_from_bonding_curve(&bonding_curve_info.data)?;
    println!("Token Creator: {}", creator);

    // Derive creator vault PDA
    let (creator_vault, _) = get_creator_vault_pda(&creator);
    println!("Creator Vault: {}", creator_vault);

    println!("\nBuilding sell instruction...");
    println!("  Amount: {} tokens", token_amount);
    println!(
        "  Min SOL output: {} SOL",
        min_sol_output as f64 / LAMPORTS_PER_SOL as f64
    );

    // Create sell instruction
    let sell_ix = create_sell_instruction(
        SellAccounts {
            global: *GLOBAL_ADDRESS,
            fee_recipient,
            mint,
            bonding_curve,
            associated_bonding_curve,
            associated_user,
            user: user.pubkey(),
            system_program: system_program::ID,
            creator_vault,
            token_program: token_program_id,
            event_authority: *EVENT_AUTHORITY,
            program: *PUMP_PROGRAM_ID,
            fee_config: *FEE_CONFIG,
            fee_program: *FEE_PROGRAM,
        },
        SellArgs {
            amount: token_amount,
            min_sol_output: min_sol_output,
        },
    );

    // Get latest blockhash
    let blockhash = connection.get_latest_blockhash()?;

    // Build transaction
    let transaction = Transaction::new_signed_with_payer(
        &[sell_ix],
        Some(&user.pubkey()),
        &[&user],
        blockhash,
    );

    // Simulate transaction
    println!("\nSimulating transaction...");
    
    match connection.send_transaction(&transaction) {
        Ok(signature) => {
            println!("Transaction sent: {}", signature);
        }
        Err(e) => {
            println!("Failed to send transaction: {}", e);
        }
    }

    // match connection.simulate_transaction(&transaction) {
    //     Ok(simulation) => {
    //         println!("Simulation result:");
    //         println!("  Error: {:?}", simulation.value.err);
    //         println!("  Logs:");
    //         if let Some(logs) = &simulation.value.logs {
    //             for log in logs {
    //                 println!("    {}", log);
    //             }
    //         }
    //         println!("  Units consumed: {:?}", simulation.value.units_consumed);

    //         if simulation.value.err.is_none() {
    //             println!("\n✓ Simulation successful! Ready to send transaction.");

    //             // Uncomment below to actually send the transaction:
    //             // println!("\nSending transaction...");
    //             // let signature = connection.send_and_confirm_transaction(&transaction)?;
    //             // println!("✓ Sell successful!");
    //             // println!("Signature: {}", signature);
    //             // println!("View on Solscan: https://solscan.io/tx/{}", signature);
    //         }
    //     }
    //     Err(e) => {
    //         println!("✗ Failed to simulate transaction: {}", e);
    //     }
    // }

    Ok(())
}


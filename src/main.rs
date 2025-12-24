mod cal;
mod pump_buy;
mod pump_sell;

use solana_client::rpc_client::RpcClient;
use solana_sdk::{native_token::LAMPORTS_PER_SOL, pubkey::Pubkey};
use std::str::FromStr;

fn test_trade() {
    let rpc = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());
    
    let mint = Pubkey::from_str("Ar4vi1BZXHVgQFRYD8AF7rBe7gsh3D1nM2hZG153pump").unwrap();
    
    println!("=== Testing Calculations for Mint: {} ===\n", mint);
    
    match cal::fetch_bonding_curve(&rpc, &mint) {
        Ok(bc) => {
            println!("Bonding Curve Data:");
            println!("  Virtual Token Reserves: {}", bc.virtual_token_reserves);
            println!("  Virtual SOL Reserves: {} lamports ({:.4} SOL)", 
                bc.virtual_sol_reserves, 
                bc.virtual_sol_reserves as f64 / 1_000_000_000.0
            );
            println!("  Real Token Reserves: {}", bc.real_token_reserves);
            println!("  Creator: {}", bc.creator);
            println!();
            
            let global = cal::Global::default();
            let sol_amount = (0.1*LAMPORTS_PER_SOL as f64) as u64;
            // Test buying with different SOL amounts
            println!("--- BUY Calculations ---");

            let tokens = cal::get_tokens_for_sol(&global, Some(&bc), sol_amount);
            println!("0.1 SOL -> {} tokens", tokens);

            let _ =  pump_buy::run_pump_buy(tokens, mint, sol_amount);

            let sol_get = cal::get_sol_for_tokens(&global, Some(&bc), tokens);
            println!("{} tokens -> {} SOL", tokens, sol_get as f64 / LAMPORTS_PER_SOL as f64);

            
            // Test selling different token amounts
            // println!("--- SELL Calculations ---");
            // for tokens_m in [1.0, 10.0, 100.0, 1000.0] {
            //     let tokens = (tokens_m * 1_000_000_000_000.0) as u64; // M tokens with 6 decimals
            //     let sol = cal::get_sol_from_tokens(&global, Some(&bc), tokens);
            //     println!(
            //         "  {:.0}M tokens -> {} lamports ({:.6} SOL)",
            //         tokens_m,
            //         sol,
            //         sol as f64 / 1_000_000_000.0
            //     );
            // }
            
            // println!();
            
            // // Test inverse: how much SOL to buy X tokens
            // println!("--- SOL NEEDED TO BUY ---");
            // for tokens_m in [1.0, 10.0, 100.0] {
            //     let tokens = (tokens_m * 1_000_000_000_000.0) as u64;
            //     let sol_needed = cal::get_sol_for_tokens(&global, Some(&bc), tokens);
            //     println!(
            //         "  {:.0}M tokens requires {} lamports ({:.6} SOL)",
            //         tokens_m,
            //         sol_needed,
            //         sol_needed as f64 / 1_000_000_000.0
            //     );
            // }
        }
        Err(e) => {
            println!("Failed to fetch bonding curve: {}", e);
            println!("Token might have migrated to Raydium or doesn't exist.");
        }
    }
}

fn main() {
    println!("Starting Pump.fun Trading Bot...\n");
   test_trade();

    // Run sell
    // if let Err(e) = pump_sell::run_pump_sell() {
    //     eprintln!("Sell Error: {}", e);
    //     std::process::exit(1);
    // }
}

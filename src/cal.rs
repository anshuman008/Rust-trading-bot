use anyhow::{anyhow, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

/// Global state from pump.fun program
#[derive(Debug, Clone)]
pub struct Global {
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
    pub creator_fee_basis_points: u64,
}

/// Bonding curve state
#[derive(Debug, Clone)]
pub struct BondingCurve {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
}

impl Default for Global {
    fn default() -> Self {
        // Default pump.fun global values
        Self {
            initial_virtual_token_reserves: 1_073_000_000_000_000, // 1.073B tokens
            initial_virtual_sol_reserves: 30_000_000_000,          // 30 SOL in lamports
            initial_real_token_reserves: 793_100_000_000_000,      // 793.1M tokens
            token_total_supply: 1_000_000_000_000_000,             // 1B tokens
            fee_basis_points: 100,                                  // 1%
            creator_fee_basis_points: 100,                          // 1%
        }
    }
}

/// Create a new bonding curve from global state
pub fn new_bonding_curve(global: &Global) -> BondingCurve {
    BondingCurve {
        virtual_token_reserves: global.initial_virtual_token_reserves,
        virtual_sol_reserves: global.initial_virtual_sol_reserves,
        real_token_reserves: global.initial_real_token_reserves,
        real_sol_reserves: 0,
        token_total_supply: global.token_total_supply,
        complete: false,
        creator: Pubkey::default(),
    }
}

/// Ceiling division: ceil(a / b)
fn ceil_div(a: u128, b: u128) -> u128 {
    (a + b - 1) / b
}

/// Compute fee based on basis points (1 basis point = 0.01%)
fn compute_fee(amount: u64, fee_basis_points: u64) -> u64 {
    ceil_div(amount as u128 * fee_basis_points as u128, 10_000) as u64
}

/// Get total fee (platform fee + creator fee if applicable)
fn get_fee(
    global: &Global,
    bonding_curve: &BondingCurve,
    amount: u64,
    is_new_bonding_curve: bool,
) -> u64 {
    let platform_fee = compute_fee(amount, global.fee_basis_points);
    let creator_fee = if is_new_bonding_curve || bonding_curve.creator != Pubkey::default() {
        compute_fee(amount, global.creator_fee_basis_points)
    } else {
        0
    };
    platform_fee + creator_fee
}

/// Calculate how many tokens you receive for a given SOL amount (BUY)
/// Returns the token amount you'll receive after fees
pub fn get_tokens_for_sol(
    global: &Global,
    bonding_curve: Option<&BondingCurve>,
    sol_amount: u64, // in lamports
) -> u64 {
    if sol_amount == 0 {
        return 0;
    }

    let (curve, is_new) = match bonding_curve {
        Some(bc) => (bc.clone(), false),
        None => (new_bonding_curve(global), true),
    };

    // Migrated bonding curve check
    if curve.virtual_token_reserves == 0 {
        return 0;
    }

    // Deduct fees from input SOL
    let fee = get_fee(global, &curve, sol_amount, is_new);
    let sol_after_fee = sol_amount.saturating_sub(fee);

    if sol_after_fee == 0 {
        return 0;
    }

    // Constant product formula: tokens_out = (virtual_token_reserves * sol_in) / (virtual_sol_reserves + sol_in)
    let tokens_out = (curve.virtual_token_reserves as u128 * sol_after_fee as u128)
        / (curve.virtual_sol_reserves as u128 + sol_after_fee as u128);

    // Cap at real token reserves
    std::cmp::min(tokens_out as u64, curve.real_token_reserves)
}

/// Calculate SOL cost for buying a specific token amount (BUY - inverse)
/// Returns total SOL needed including fees
pub fn get_sol_for_tokens(
    global: &Global,
    bonding_curve: Option<&BondingCurve>,
    token_amount: u64,
) -> u64 {
    if token_amount == 0 {
        return 0;
    }

    let (curve, is_new) = match bonding_curve {
        Some(bc) => (bc.clone(), false),
        None => (new_bonding_curve(global), true),
    };

    // Migrated bonding curve check
    if curve.virtual_token_reserves == 0 {
        return 0;
    }

    // Cap token amount at available reserves
    let min_amount = std::cmp::min(token_amount, curve.real_token_reserves);

    // Constant product formula (inverse): sol_cost = (virtual_sol_reserves * tokens) / (virtual_token_reserves - tokens) + 1
    let denominator = curve.virtual_token_reserves.saturating_sub(min_amount);
    if denominator == 0 {
        return u64::MAX; // Would require all tokens
    }

    let sol_cost = (curve.virtual_sol_reserves as u128 * min_amount as u128)
        / denominator as u128
        + 1;

    let sol_cost = sol_cost as u64;

    // Add fees
    sol_cost + get_fee(global, &curve, sol_cost, is_new)
}

/// Calculate how much SOL you receive for selling tokens (SELL)
/// Returns SOL amount after fees
pub fn get_sol_from_tokens(
    global: &Global,
    bonding_curve: Option<&BondingCurve>,
    token_amount: u64,
) -> u64 {
    if token_amount == 0 {
        return 0;
    }

    let (curve, is_new) = match bonding_curve {
        Some(bc) => (bc.clone(), false),
        None => (new_bonding_curve(global), true),
    };

    // Migrated bonding curve check
    if curve.virtual_token_reserves == 0 || curve.virtual_sol_reserves == 0 {
        return 0;
    }

    // Constant product formula: sol_out = (virtual_sol_reserves * tokens_in) / (virtual_token_reserves + tokens_in)
    let sol_out = (curve.virtual_sol_reserves as u128 * token_amount as u128)
        / (curve.virtual_token_reserves as u128 + token_amount as u128);

    let sol_out = sol_out as u64;

    // Deduct fees
    let fee = get_fee(global, &curve, sol_out, is_new);
    sol_out.saturating_sub(fee)
}

/// Parse bonding curve data from on-chain account
/// Layout: 8 (discriminator) + 8 (virtual_token_reserves) + 8 (virtual_sol_reserves) +
///         8 (real_token_reserves) + 8 (real_sol_reserves) + 8 (token_total_supply) +
///         1 (complete) + 32 (creator)
pub fn parse_bonding_curve(data: &[u8]) -> Result<BondingCurve> {
    if data.len() < 81 {
        return Err(anyhow!("Bonding curve data too short: {} bytes", data.len()));
    }

    let virtual_token_reserves = u64::from_le_bytes(data[8..16].try_into().unwrap());
    let virtual_sol_reserves = u64::from_le_bytes(data[16..24].try_into().unwrap());
    let real_token_reserves = u64::from_le_bytes(data[24..32].try_into().unwrap());
    let real_sol_reserves = u64::from_le_bytes(data[32..40].try_into().unwrap());
    let token_total_supply = u64::from_le_bytes(data[40..48].try_into().unwrap());
    let complete = data[48] != 0;
    let creator = Pubkey::new_from_array(data[49..81].try_into().unwrap());

    Ok(BondingCurve {
        virtual_token_reserves,
        virtual_sol_reserves,
        real_token_reserves,
        real_sol_reserves,
        token_total_supply,
        complete,
        creator,
    })
}

lazy_static::lazy_static! {
    static ref PUMP_PROGRAM_ID: Pubkey = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P").unwrap();
}

/// Derive the bonding curve PDA for a mint
pub fn get_bonding_curve_pda(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &PUMP_PROGRAM_ID)
}

/// Fetch and parse bonding curve from RPC
pub fn fetch_bonding_curve(rpc: &RpcClient, mint: &Pubkey) -> Result<BondingCurve> {
    let (bonding_curve_pda, _) = get_bonding_curve_pda(mint);
    let account = rpc
        .get_account(&bonding_curve_pda)
        .map_err(|e| anyhow!("Failed to fetch bonding curve: {}", e))?;
    parse_bonding_curve(&account.data)
}

/// Calculate buy quote: SOL -> Tokens
/// Returns (tokens_received, sol_after_fees, fee_amount)
pub fn quote_buy(
    rpc: &RpcClient,
    mint: &Pubkey,
    sol_amount: u64,
) -> Result<(u64, u64, u64)> {
    let bonding_curve = fetch_bonding_curve(rpc, mint)?;
    let global = Global::default();

    let tokens = get_tokens_for_sol(&global, Some(&bonding_curve), sol_amount);
    let fee = get_fee(&global, &bonding_curve, sol_amount, false);
    let sol_after_fee = sol_amount.saturating_sub(fee);

    Ok((tokens, sol_after_fee, fee))
}

/// Calculate sell quote: Tokens -> SOL
/// Returns (sol_received, fee_amount)
pub fn quote_sell(
    rpc: &RpcClient,
    mint: &Pubkey,
    token_amount: u64,
) -> Result<(u64, u64)> {
    let bonding_curve = fetch_bonding_curve(rpc, mint)?;
    let global = Global::default();

    // Calculate gross SOL (before fees)
    let gross_sol = (bonding_curve.virtual_sol_reserves as u128 * token_amount as u128)
        / (bonding_curve.virtual_token_reserves as u128 + token_amount as u128);
    let gross_sol = gross_sol as u64;

    let fee = get_fee(&global, &bonding_curve, gross_sol, false);
    let net_sol = gross_sol.saturating_sub(fee);

    Ok((net_sol, fee))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buy_calculation() {
        let global = Global::default();
        let bonding_curve = new_bonding_curve(&global);

        // Buy with 1 SOL (1_000_000_000 lamports)
        let sol_amount = 1_000_000_000;
        let tokens = get_tokens_for_sol(&global, Some(&bonding_curve), sol_amount);

        println!("Buying with {} lamports", sol_amount);
        println!("Tokens received: {}", tokens);
        assert!(tokens > 0);
    }

    #[test]
    fn test_sell_calculation() {
        let global = Global::default();
        let bonding_curve = new_bonding_curve(&global);

        // Sell 1M tokens
        let token_amount = 1_000_000_000_000; // 1M tokens (6 decimals)
        let sol = get_sol_from_tokens(&global, Some(&bonding_curve), token_amount);

        println!("Selling {} tokens", token_amount);
        println!("SOL received: {} lamports", sol);
        assert!(sol > 0);
    }

    #[test]
    fn test_inverse_calculation() {
        let global = Global::default();
        let bonding_curve = new_bonding_curve(&global);

        // Get SOL cost for buying specific token amount
        let desired_tokens = 1_000_000_000_000; // 1M tokens
        let sol_needed = get_sol_for_tokens(&global, Some(&bonding_curve), desired_tokens);

        println!("To buy {} tokens, need {} lamports", desired_tokens, sol_needed);
        assert!(sol_needed > 0);
    }
}


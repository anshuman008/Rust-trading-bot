# Pump.fun Trading Bot (Rust)

A high-performance Rust implementation for trading tokens on [pump.fun](https://pump.fun) - the Solana memecoin launchpad.

## Features

- **Buy Tokens** - Purchase tokens from pump.fun bonding curves
- **Sell Tokens** - Sell tokens back to the bonding curve for SOL
- **Price Calculations** - Calculate expected token amounts and SOL costs before trading
- **Real-time Bonding Curve Data** - Fetch live on-chain state

## Project Structure

```
src/
├── main.rs        # Entry point and test functions
├── pump_buy.rs    # Buy instruction builder and executor
├── pump_sell.rs   # Sell instruction builder and executor
└── cal.rs         # Bonding curve calculations (buy/sell quotes)
```

## Installation

```bash
# Clone the repo
git clone <repo-url>
cd trading-bot-rust

# Build
cargo build --release

# Run
cargo run
```

## Configuration

Edit the constants in `pump_buy.rs` and `pump_sell.rs`:

```rust
const PRIVATE_KEY: &str = "your-base58-private-key";
const MINT_ADDRESS: &str = "token-mint-address";
const TOKEN_AMOUNT: u64 = 1000;
const MAX_SOL_COST: u64 = 1_000_000;  // For buy (lamports)
const MIN_SOL_OUTPUT: u64 = 0;         // For sell (slippage protection)
```

## Usage

### Calculate Buy/Sell Quotes

```rust
use crate::cal::{Global, fetch_bonding_curve, get_tokens_for_sol, get_sol_from_tokens};

let rpc = RpcClient::new("https://api.mainnet-beta.solana.com");
let mint = Pubkey::from_str("your-token-mint")?;
let bonding_curve = fetch_bonding_curve(&rpc, &mint)?;
let global = Global::default();

// How many tokens for 1 SOL?
let tokens = get_tokens_for_sol(&global, Some(&bonding_curve), 1_000_000_000);

// How much SOL for selling 1M tokens?
let sol = get_sol_from_tokens(&global, Some(&bonding_curve), 1_000_000_000_000);

// How much SOL needed to buy 1M tokens?
let sol_needed = get_sol_for_tokens(&global, Some(&bonding_curve), 1_000_000_000_000);
```

### Execute Buy

```rust
use crate::pump_buy::run_pump_buy;

// Configure MINT_ADDRESS and other constants, then:
run_pump_buy(token_amount, mint, max_sol_cost)?;
```

### Execute Sell

```rust
use crate::pump_sell::run_pump_sell;

// Configure MINT_ADDRESS and other constants, then:
run_pump_sell()?;
```

## Calculation Functions

| Function | Description |
|----------|-------------|
| `get_tokens_for_sol(global, bc, sol)` | Tokens received for X SOL (buy) |
| `get_sol_for_tokens(global, bc, tokens)` | SOL needed to buy X tokens |
| `get_sol_from_tokens(global, bc, tokens)` | SOL received for selling X tokens |
| `quote_buy(rpc, mint, sol)` | Quick buy quote with RPC fetch |
| `quote_sell(rpc, mint, tokens)` | Quick sell quote with RPC fetch |

## Bonding Curve Math

Pump.fun uses a **constant product AMM** similar to Uniswap:

```
Buy:  tokens_out = (virtual_token_reserves * sol_in) / (virtual_sol_reserves + sol_in)
Sell: sol_out = (virtual_sol_reserves * tokens_in) / (virtual_token_reserves + tokens_in)
```

### Fee Structure
- **Platform Fee**: 1% (100 basis points)
- **Creator Fee**: 1% (100 basis points) - if creator is set

## Program Addresses

| Account | Address |
|---------|---------|
| Pump Program | `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P` |
| Global State | `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf` |
| Event Authority | `Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1` |
| Fee Program | `pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ` |

## Dependencies

```toml
solana-sdk = "2.1"
solana-client = "2.1"
spl-token = "7.0"
spl-token-2022 = "6.0"
spl-associated-token-account = "6.0"
bs58 = "0.5"
anyhow = "1.0"
lazy_static = "1.5"
```

## Testing

```bash
# Run unit tests
cargo test -- --nocapture

# Test with real RPC data (read-only)
cargo run
```

## Security Notes

⚠️ **Never commit your private key** to version control. Use environment variables or a secure config file.

```rust
// Better approach:
let private_key = std::env::var("PUMP_PRIVATE_KEY")
    .expect("PUMP_PRIVATE_KEY not set");
```

## License

MIT


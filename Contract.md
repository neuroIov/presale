# NLOV Token Presale Contract 
## 1. Project Overview

The presale program is a Solana smart contract designed to manage a multi-stage token presale event for the **NLOV** token. It facilitates token purchases using both native **SOL** and **SPL stablecoins** (USDC and USDT), enforces a **hardcap** on total tokens sold, and includes mechanisms for managing **sale stages** and transferring unsold tokens to a liquidity pool post-sale.

### Sale Phases:
- **Phase 0**: Not Started — Initial state after initialization.
- **Phase 1**: Private Sale — Tokens can be purchased by eligible participants.
- **Phase 2**: Public Sale — Open to all participants.
- **Phase 3**: Ended — Sale concluded; unsold tokens moved to liquidity pool.

The contract supports both **Web3 (on-chain)** and **Web2 (off-chain - exchange)** payment tracking.

---

## 2. Contract Architecture

### Key Accounts:

| Account | Description |
|--------|-------------|
| **Presale PDA** | Stores mutable state, derived using `PRESALE_SEED` + admin pubkey. |
| **admin** | Authorized initializer and controller of sale. |
| **token_mint** | SPL Mint of the NLOV token. |
| **presale_wallet** | Holds NLOV tokens for sale, controlled by the PDA. |
| **merchant_wallet** | Receives SOL payments. |
| **buyer** | Purchaser of tokens. |
| **buyer_stable_coin_account** | Holds buyer’s USDC/USDT. |
| **merchant_stable_coin_account** | Receives USDC/USDT payments. |
| **stable_coin_mint** | USDC or USDT mint. |
| **liquidity_wallet** | Receives unsold NLOV tokens after sale ends. |

---

## 3. Program Instructions

### `initialize`

Initializes the contract with pricing, durations, hardcap, and key wallets.

- **Params**: 
  - `usd_price_cents_per_nlov: u64`
  - `sol_price_lamports_per_nlov: u64`
  - `private_sale_duration_days: i64`
  - `public_sale_duration_days: i64`
  - `hardcap_tokens: u64`
- **Accounts**: `admin`, `presale`, `token_mint`, `presale_wallet`, `merchant_wallet`, etc.

---

### `set_stage`

Transitions the presale stage forward sequentially:
- From `NotStarted → Private → Public → Ended`.
- Checks for correct durations before transitions.

---

### `update_sale_period`

Allows admin to adjust sale durations.

- **Params**: 
  - `new_private_sale_duration_days`
  - `new_public_sale_duration_days`

---

### `buy_tokens`

Allows users to purchase tokens using **SOL**.

- **Params**:
  - `payment_type: u8` (`0 = Web3`, `1 = Web2`)
  - `lamports_sent: u64`
- **Checks**:
  - Sale must be active.
  - Hardcap not exceeded.
  - Correct SOL amount based on price.

---

### `buy_tokens_by_stable_coin`

Same as `buy_tokens`, but for **USDC/USDT** purchases.

- **Params**:
  - `payment_type: u8`
  - `stable_coin_amount_user_units: u64`
- **Checks**:
  - Valid stablecoin.
  - Sale must be active.
  - Hardcap not exceeded.

---

### `check_presale_token_balance`

Returns available tokens remaining in `presale_wallet`.

---

### `finalize_presale`

Transfers unsold tokens to `liquidity_wallet`.

- Only executable by admin.
- Only after sale is ended.
- Prevents duplicate finalizations via `pool_created` flag.

---

## 4. State Management (`Presale` Account Struct)

| Field | Type | Description |
|-------|------|-------------|
| `admin` | `Pubkey` | Contract owner |
| `presale_start` | `i64` | Timestamp |
| `usd_price_cents_per_nlov` | `u64` | Price in cents |
| `sol_price_lamports_per_nlov` | `u64` | Price in lamports |
| `private_sale_duration` | `i64` | Seconds |
| `public_sale_duration` | `i64` | Seconds |
| `sale_stage` | `u8` | 0-3 for each stage |
| `total_sold` | `u64` | Tokens sold |
| `hardcap_tokens` | `u64` | Max tokens for sale |
| `pool_created` | `bool` | If liquidity pool is created |
| `presale_wallet` | `Pubkey` | Token source |
| `merchant_wallet` | `Pubkey` | Payment recipient |
| `bump` | `u8` | PDA bump |

---

## 5. Error Handling 

- `InvalidTokenAccount`
- `PrivateSaleNotOver`
- `PublicSaleNotOver`
- `SaleAlreadyEnded`
- `PresaleNotActive`
- `InsufficientTokens`
- `InvalidStableToken`
- `InvalidPaymentType`
- `InvalidPrice`
- `Unauthorized`
- `LiquidityPoolAlreadyCreated`
- `HardcapReached`

---

## 6. Tokenomics

- **Token**: `NLOV`
- **Decimals**: Based on actual mint
- **Pricing**:
  - 1 NLOV = `$0.03` USD
 
- **Supply Control**:
  - Hardcap-enforced sales
  - Unsold tokens moved to liquidity wallet

---

## 7. Dependencies

- `anchor-lang` `v0.31.1`
  - Features: `derive`
- `anchor-spl` `v0.31.1`
  - Features: `spl-token`, `metadata`
- `solana_program` (via `anchor_lang::solana_program`)

---

## 8. Security Considerations

- **Access Control**: Admin-only for sensitive ops
- **PDA Authority**: Ensures secure ownership
- **Re-entrancy**: Solana's model prevents this
- **Overflow Checks**: `checked_add`, `checked_mul`, etc.
- **Hardcap**: Enforced at time of purchase
- **Single Finalization**: Prevented via `pool_created` flag

---

## 9. Deployment Information

- **Cluster**: `Solana Devnet`  
  `https://api.devnet.solana.com`
- **Program ID**:  
  `Duf9UdBXfrxgBeZgZ2DUxRgFSZ4qCzEgGyxFmuQHGHZH`
- **Admin Wallet**:  
  Deployed using `id.json`
- **IDL File**:  
  `target/idl/presale.json`

---

## 10. Test Suite

Tested using `solana-program-test` in `tests/integration.rs`.

### Coverage:
- Initialization
- Stage Transitions
- SOL Purchases
- Stablecoin Purchases
- Price Updates
- Sale Period Updates
- Hardcap Enforcement
- Finalization
- Unauthorized Access
- Event Emission

---
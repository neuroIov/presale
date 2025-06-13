

```markdown
#  NLOV Presale  Contract

## Overview

This repo contains the  smart contract for the **NLOV token presale**, built using the Anchor framework. It facilitates a **multi-stage token sale**, allowing participants to purchase NLOV tokens using either native **SOL** or SPL stablecoins (**USDC/USDT**). Supports flexible pricing, stage management, token caps, and post-sale token handling.



##  Features

- Multi-Stage Presale: Configurable _Private Sale_ and _Public Sale_ phases with independent durations.
- Flexible Pricing:
  - `1 NLOV = $0.03 USD` (USDC/USDT)
  - `1 NLOV = 0.182 SOL`
- Dual Payment Modes:
  - Web3: Direct SOL or stablecoin payments on-chain.
  - Web2: Off-chain payments with on-chain sale record tracking{ Exchnage wallets}
- Token Hardcap:Maximum cap on total NLOV tokens sold during the entire presale.
- Admin Controls:
  - Initialize presale
  - Update pricing, timings
  - Manage sale stages
- Unsold Token Management:After presale ends, remaining tokens can be transferred to a liquidity wallet for Public IDO/TGE.
- Audit-Ready Architecture:Strong error handling, secure access control, and test coverage to support auditing.


## Setup 

### 1. Install Solana CLI v1.18.26
### 2. Install Rust v1.87.0
### 3. Install Anchor CLI v0.31.1
### 4. Clone the Repository
### 5. Build the Program

This will compile the contract and generate the IDL at `target/idl/presale.json`.

### 6. Run Tests

All tests should pass before deployment.

## Deployment Status

The contract is currently deployed to **Solana Devnet**.

* **Program ID:** `Duf9UdBXfrxgBeZgZ2DUxRgFSZ4qCzEgGyxFmuQHGHZH`
* [ Solana Explorer](https://explorer.solana.com/address/Duf9UdBXfrxgBeZgZ2DUxRgFSZ4qCzEgGyxFmuQHGHZH?cluster=devnet)


##  Usage 

Frontend interaction can be done via  **@solana/web3.js** or **Anchor TS client**

###  Instructions

* **`initialize`** – Admin initializes the presale and sets the initial config.
* **`set_stage`** – Admin updates the sale stage (Private → Public → Ended).
* **`buy_tokens`** – Users purchase NLOV using SOL.
* **`buy_tokens_by_stable_coin`** – Users purchase NLOV using USDC/USDT.
* **`finalize_presale`** – Admin finalizes the presale and transfers unsold tokens to a liquidity wallet.

IDL is available at:


target/idl/presale.json

Use it to build TypeScript or web clients.


###  Contract Documentation

A detailed breakdown of  design, access control, program flows, and test coverage is available:

📄 [View Full Contract Docs →](./contract.md)





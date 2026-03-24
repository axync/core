# Axync Core

Rust backend for [Axync](https://axync.xyz) — cross-chain marketplace for tokens and vesting positions.

## What It Does

Axync Core is the off-chain settlement engine. It runs a sequencer that batches transactions into blocks, generates ZK proofs of state transitions, and coordinates with on-chain contracts for trustless cross-chain trading.

```
On-chain Events ──> Watcher ──> Sequencer ──> Prover ──> Relayer ──> On-chain Verification
```

- **Watcher** listens for deposits (AxyncVault) and listings (AxyncEscrow) on each chain
- **Sequencer** processes transactions, builds blocks every 5 seconds
- **Prover** generates merkle roots and ZK proofs for each block
- **API** exposes REST endpoints for the frontend and relayer

## Crates

| Crate | Description |
|-------|-------------|
| `axync-types` | Shared types: Address, Block, Transaction, chain IDs |
| `axync-state` | Account state: balances, nonces, listings |
| `axync-stf` | State transition function: validates and applies transactions |
| `axync-sequencer` | Block production, transaction queue, execution |
| `axync-prover` | Keccak256 merkle trees, ZK proof generation |
| `axync-storage` | RocksDB (production) and in-memory (testing) storage |
| `axync-watcher` | Multi-chain event listener with reorg detection |
| `axync-api` | Axum HTTP server with REST API |

## Quick Start

```bash
cp .env.example .env
# Edit .env with RPC URLs and contract addresses

cargo build --release
./target/release/axync-api
```

API starts on `http://localhost:8080`.

## API

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check |
| GET | `/api/v1/account/:address` | Account state (balances, nonce) |
| GET | `/api/v1/nft-listings` | All marketplace listings |
| GET | `/api/v1/nft-listing/:id` | Single listing |
| POST | `/api/v1/transactions` | Submit signed transaction (BuyNft) |
| GET | `/api/v1/nft-release-proof/:id` | Merkle proof for claiming |
| GET | `/api/v1/withdrawal-proof/:addr/:asset/:amount/:chain` | Withdrawal proof |
| GET | `/api/v1/current_block` | Current block ID |
| GET | `/api/v1/block/:id` | Block details |
| GET | `/api/v1/chains` | Supported chains |

## Docker

```bash
docker build -t axync/core:latest .
docker run -p 8080:8080 --env-file .env axync/core:latest
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PORT` | API port (default: 8080) |
| `BLOCK_INTERVAL_SEC` | Block production interval (default: 5) |
| `ETHEREUM_RPC_URL` | Ethereum Sepolia RPC |
| `BASE_RPC_URL` | Base Sepolia RPC |
| `ETHEREUM_DEPOSIT_CONTRACT` | AxyncVault address on Ethereum |
| `ETHEREUM_ESCROW_CONTRACT` | AxyncEscrow address on Ethereum |
| `BASE_DEPOSIT_CONTRACT` | AxyncVault address on Base |
| `BASE_ESCROW_CONTRACT` | AxyncEscrow address on Base |
| `POLL_INTERVAL_SECONDS` | Chain polling interval (default: 15) |
| `USE_PLACEHOLDER_PROVER` | Use placeholder proofs (default: true) |

## Supported Chains

| Chain | Chain ID | Status |
|-------|----------|--------|
| Ethereum Sepolia | 11155111 | Live |
| Base Sepolia | 84532 | Live |
| Ethereum | 1 | Planned |
| Base | 8453 | Planned |

## License

MIT

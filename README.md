# Axync Core

High-performance off-chain settlement engine for trustless cross-chain OTC trading, powered by zero-knowledge proofs.

## Overview

Axync Core is the backend engine that orchestrates trustless over-the-counter (OTC) trading across multiple blockchains. It operates as an off-chain sequencer that batches transactions into blocks, generates cryptographic proofs of state transitions, and publishes succinct proofs on-chain for verification.

The system uses a two-layer proving architecture:
1. **STARK proofs** verify the correctness of state transitions (deposits, withdrawals, deal creation/acceptance/cancellation).
2. **Groth16 SNARK proofs** wrap STARK proofs into compact proofs suitable for on-chain verification on Ethereum and other EVM-compatible chains.

## Architecture

```
                          +-------------------+
                          |   Smart Contracts |
                          | (Deposit/Withdraw)|
                          +--------+----------+
                                   |
                          +--------v----------+
                          |      Watcher      |
                          | (Chain Listeners) |
                          +--------+----------+
                                   |
                 +-----------------v-----------------+
                 |            Sequencer               |
                 |  (Transaction Queue + Block Prod.) |
                 +---------+-----------+-------------+
                           |           |
                  +--------v--+   +----v--------+
                  |    STF     |   |   Prover    |
                  | (State     |   | (STARK +    |
                  | Transition)|   |  SNARK)     |
                  +-----+------+   +------+------+
                        |                 |
                  +-----v------+   +------v------+
                  |   State    |   |   Storage   |
                  | (Accounts, |   | (RocksDB /  |
                  |  Deals)    |   |  In-Memory) |
                  +-----+------+   +-------------+
                        |
                  +-----v------+
                  |   Types    |
                  | (Shared    |
                  |  Primitives)|
                  +------------+
```

## Crates

| Crate | Description |
|-------|-------------|
| `axync-types` | Shared type definitions: `Address`, `Block`, `Tx`, `Deal`, `Balance`, chain ID constants, and all transaction payload types. |
| `axync-state` | In-memory state representation holding accounts, balances, deals, and the account address index. |
| `axync-stf` | State Transition Function: applies transactions (Deposit, Withdraw, CreateDeal, AcceptDeal, CancelDeal) to state with nonce validation and balance checks. |
| `axync-sequencer` | Transaction queue management, block building, block execution, snapshot persistence, and optional ZK proof generation coordination. |
| `axync-prover` | Two-layer ZK proving system: a minimal STARK prover for state transition verification and a Groth16 SNARK wrapper (via Arkworks) for on-chain verification. Includes Merkle tree, nullifier, and withdrawal proof utilities. |
| `axync-storage` | Persistent storage backends: `InMemoryStorage` for testing and `RocksDBStorage` for production. Stores blocks, transactions, deals, and state snapshots. |
| `axync-watcher` | Multi-chain deposit watcher: listens to on-chain deposit events via JSON-RPC polling with reorg detection, retry logic, and deduplication. |
| `axync-api` | Axum-based HTTP API server exposing REST and JSON-RPC endpoints for submitting transactions, querying state, and monitoring health. Includes rate limiting middleware. |
| `axync-demo` | Interactive demonstration of the complete user flow: deposit, deal creation, deal acceptance, block production with proofs, and withdrawal. |

## Prerequisites

- **Rust** 1.70+ (stable toolchain)
- **Clang/LLVM** (required for RocksDB compilation)
- **pkg-config** and **libssl-dev** (Linux)
- **Docker** and **Docker Compose** (optional, for containerized deployment)

## Build

```bash
# Build all crates (default features: RocksDB enabled for API)
cargo build --release

# Build without RocksDB (uses in-memory storage)
cargo build --release -p axync-api --no-default-features

# Build with STARK prover enabled
cargo build --release --features stark

# Build with Arkworks Groth16 SNARK prover
cargo build --release --features arkworks

# Build with all proof features
cargo build --release --features "stark,arkworks"
```

## Running

### Local Development

```bash
# Copy environment configuration
cp .env.example .env

# Run the API server
cargo run --release -p axync-api

# Run the demo
cargo run --release -p axync-demo
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Logging level (`debug`, `info`, `warn`, `error`) |
| `PORT` | `8080` | API server port |
| `STORAGE_PATH` | `./data` | Path for RocksDB persistent storage |
| `BLOCK_INTERVAL_SEC` | `1` | Block production interval in seconds |
| `MAX_QUEUE_SIZE` | `10000` | Maximum transaction queue size |
| `MAX_TXS_PER_BLOCK` | `100` | Maximum transactions per block |
| `USE_PLACEHOLDER_PROVER` | `true` | Use placeholder proofs (set `false` for real ZK proofs) |
| `GROTH16_KEYS_DIR` | `./crates/prover/keys` | Directory for Groth16 proving/verifying keys |
| `FORCE_REGENERATE_KEYS` | `false` | Force regeneration of Groth16 keys on startup |
| `RATE_LIMIT_MAX_REQUESTS` | `100` | Max API requests per window |
| `RATE_LIMIT_WINDOW_SECONDS` | `60` | Rate limit window duration |
| `ETHEREUM_RPC_URL` | - | Ethereum RPC endpoint for deposit watching |
| `ETHEREUM_CHAIN_ID` | `1` | Ethereum chain ID |
| `ETHEREUM_DEPOSIT_CONTRACT` | - | Ethereum deposit contract address |
| `BASE_RPC_URL` | - | Base chain RPC endpoint |
| `BASE_CHAIN_ID` | `8453` | Base chain ID |
| `BASE_DEPOSIT_CONTRACT` | - | Base deposit contract address |
| `POLL_INTERVAL_SECONDS` | `3` | Chain watcher polling interval |
| `RPC_TIMEOUT_SECONDS` | `30` | RPC request timeout |
| `REORG_SAFETY_BLOCKS` | `10` | Blocks to wait before processing (reorg safety) |

## API Endpoints

### REST API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/health` | Health check with component status |
| `GET` | `/ready` | Readiness probe (Kubernetes-compatible) |
| `GET` | `/api/v1/account/:address` | Get full account state |
| `GET` | `/api/v1/account/:address/balance/:asset_id` | Get specific asset balance |
| `GET` | `/api/v1/deals` | List deals (supports filtering) |
| `GET` | `/api/v1/deal/:deal_id` | Get deal details |
| `GET` | `/api/v1/block/:block_id` | Get block information |
| `POST` | `/api/v1/transactions` | Submit a transaction |
| `GET` | `/api/v1/queue/status` | Get transaction queue status |
| `GET` | `/api/v1/chains` | Get supported chains |

### JSON-RPC

| Method | Description |
|--------|-------------|
| `axync_submitTransaction` | Submit a signed transaction |
| `axync_getBalance` | Query account balance |
| `axync_getBlock` | Query block by ID |
| `axync_getDeal` | Query deal by ID |

All JSON-RPC requests are sent via `POST /jsonrpc` with standard JSON-RPC 2.0 envelope.

## Docker Deployment

### Production

```bash
# Build and start
docker-compose up -d

# View logs
docker-compose logs -f

# Stop
docker-compose down
```

### Development

```bash
# Start development container with source mounting
docker-compose -f docker-compose.dev.yml up -d
```

The production Docker image uses a multi-stage build to produce a minimal runtime image. Persistent data is stored in a named Docker volume (`axync-data`).

## Testing

```bash
# Run all unit tests
cargo test

# Run tests with STARK prover
cargo test --features stark

# Run tests with Arkworks SNARK prover
cargo test --features arkworks

# Run all proof-related tests
cargo test --features "stark,arkworks"

# Run a specific crate's tests
cargo test -p axync-sequencer
cargo test -p axync-prover

# Run integration tests (requires Hardhat node)
cargo test -p axync-watcher -- --ignored
```

## Supported Chains

| Chain | Chain ID | Status |
|-------|----------|--------|
| Ethereum | 1 | Supported |
| Polygon | 137 | Supported |
| Arbitrum | 42161 | Supported |
| Optimism | 10 | Supported |
| Base | 8453 | Supported |
| Mantle | 5000 | Supported |

## License

Proprietary. All rights reserved.

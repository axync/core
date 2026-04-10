# Axync API Reference

Base URL: `http://localhost:3000` (default)

All addresses are hex-encoded 20-byte Ethereum addresses (with or without `0x` prefix).
All signatures are hex-encoded 65-byte EIP-712 signatures.
Amounts are raw integers (no decimals applied). For JSON, large numbers (>2^53) must be passed as strings.

---

## Health & Status

### GET /health

Component health status.

```bash
curl http://localhost:3000/health
```

```json
{
  "status": "healthy",
  "timestamp": 1712764800,
  "components": {
    "sequencer": { "status": "healthy", "current_block_id": 5, "queue_length": 0 },
    "storage": { "status": "healthy", "configured": true }
  }
}
```

### GET /ready

Kubernetes/Docker readiness probe. Returns 200 if ready, 503 if not.

```bash
curl http://localhost:3000/ready
```

### GET /api/v1/current_block

```bash
curl http://localhost:3000/api/v1/current_block
```

```json
{ "current_block_id": 5 }
```

### GET /api/v1/queue/status

```bash
curl http://localhost:3000/api/v1/queue/status
```

```json
{
  "pending_transactions": 3,
  "max_queue_size": 10000,
  "current_block_id": 5
}
```

### GET /api/v1/chains

Supported chains and their IDs.

```bash
curl http://localhost:3000/api/v1/chains
```

```json
{
  "chains": [
    { "chain_id": 1, "name": "Ethereum" },
    { "chain_id": 137, "name": "Polygon" },
    { "chain_id": 8453, "name": "Base" },
    { "chain_id": 42161, "name": "Arbitrum" },
    { "chain_id": 10, "name": "Optimism" },
    { "chain_id": 11155111, "name": "Ethereum Sepolia" },
    { "chain_id": 84532, "name": "Base Sepolia" }
  ]
}
```

---

## Accounts

### GET /api/v1/account/:address

Full account state. Creates the account if it doesn't exist yet.

```bash
curl http://localhost:3000/api/v1/account/0x1234...abcd
```

```json
{
  "address": [1, 2, ...],
  "account_id": 0,
  "balances": [
    { "asset_id": 0, "chain_id": 1, "amount": 1000000 },
    { "asset_id": 1, "chain_id": 8453, "amount": 5000 }
  ],
  "nonce": 3,
  "open_deals": [42, 43]
}
```

### GET /api/v1/account/:address/balance/:asset_id

Balances for a specific asset across all chains.

```bash
curl http://localhost:3000/api/v1/account/0x1234...abcd/balance/0
```

```json
{
  "address": [1, 2, ...],
  "asset_id": 0,
  "balances": [
    { "chain_id": 1, "amount": 1000000 },
    { "chain_id": 8453, "amount": 500000 }
  ]
}
```

---

## Deals

### GET /api/v1/deals

List all deals. Supports query filters:

| Param | Description | Example |
|-------|-------------|---------|
| `status` | Filter by status: `Pending`, `Settled`, `Cancelled` | `?status=Pending` |
| `address` | Filter by maker or taker address | `?address=0x1234...abcd` |

```bash
# All pending deals
curl "http://localhost:3000/api/v1/deals?status=Pending"

# Deals for a specific address
curl "http://localhost:3000/api/v1/deals?address=0x1234...abcd"
```

```json
{
  "deals": [
    {
      "deal_id": 42,
      "maker": [1, 1, ...],
      "taker": null,
      "offer": {
        "type": "fungible",
        "asset_id": 1,
        "amount": 1000,
        "chain_id": 8453
      },
      "consideration": {
        "type": "fungible",
        "asset_id": 0,
        "amount": 1000000,
        "chain_id": 1
      },
      "amount_filled": 0,
      "status": "Pending",
      "created_at": 1712764800,
      "expires_at": null,
      "is_cross_chain": true
    }
  ],
  "total": 1
}
```

### GET /api/v1/deal/:deal_id

Single deal details.

```bash
curl http://localhost:3000/api/v1/deal/42
```

Response: same structure as a single element in the deals list above.

---

## Blocks

### GET /api/v1/block/:block_id

```bash
curl http://localhost:3000/api/v1/block/0
```

```json
{
  "block_id": 0,
  "transaction_count": 5,
  "timestamp": 1712764800,
  "state_root": "0x45db9d...",
  "withdrawals_root": "0x0000...",
  "block_proof": "0xabcdef...",
  "transactions": [
    { "id": 0, "from": [1, 1, ...], "nonce": 0, "kind": "Deposit" },
    { "id": 1, "from": [2, 2, ...], "nonce": 0, "kind": "Deposit" }
  ]
}
```

---

## Submit Transactions

### POST /api/v1/transactions

Submit a signed transaction. The `kind` field determines the transaction type.

All transactions require an EIP-712 `signature` (65 bytes, hex-encoded).

Response (all types):
```json
{ "tx_hash": "0x...", "status": "queued" }
```

---

### Deposit

Submitted by the watcher when an on-chain deposit is detected. Normally not called by the frontend directly.

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "Deposit",
    "tx_hash": "0xabc...def",
    "account": "0x1234...abcd",
    "asset_id": 0,
    "amount": "1000000",
    "chain_id": 1,
    "nonce": 0,
    "signature": "0x..."
  }'
```

---

### CreateDeal

Create a new deal. The maker's offer balance is **locked immediately** (subtracted from available balance).

**TradeAsset format:**

```json
// Fungible (ERC20/ETH via Vault)
{ "type": "fungible", "asset_id": 0, "amount": "1000000", "chain_id": 1 }

// Escrowed (NFT/ERC20 locked in AxyncEscrow on-chain)
{ "type": "escrowed", "escrow_listing_id": 7 }
```

**Example: Sell 1000 TOKEN (Ethereum) for 0.6 ETH (Base)**

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "CreateDeal",
    "from": "0x1234...abcd",
    "deal_id": 42,
    "visibility": "Public",
    "taker": null,
    "offer": {
      "type": "fungible",
      "asset_id": 1,
      "amount": "1000",
      "chain_id": 1
    },
    "consideration": {
      "type": "fungible",
      "asset_id": 0,
      "amount": "600000000000000000",
      "chain_id": 8453
    },
    "expires_at": null,
    "external_ref": null,
    "nonce": 2,
    "signature": "0x..."
  }'
```

**Example: Sell NFT (Escrow listing #7 on Base) for 500 USDC (Ethereum)**

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "CreateDeal",
    "from": "0x1234...abcd",
    "deal_id": 43,
    "visibility": "Public",
    "taker": null,
    "offer": {
      "type": "escrowed",
      "escrow_listing_id": 7
    },
    "consideration": {
      "type": "fungible",
      "asset_id": 0,
      "amount": "500000000",
      "chain_id": 1
    },
    "expires_at": 1712851200,
    "external_ref": null,
    "nonce": 3,
    "signature": "0x..."
  }'
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `from` | hex string | yes | Maker's address |
| `deal_id` | u64 | yes | Unique deal identifier (client-generated) |
| `visibility` | string | yes | `"Public"` or `"Direct"` |
| `taker` | hex string | no | Required if visibility is `"Direct"` |
| `offer` | TradeAsset | yes | What the maker is selling |
| `consideration` | TradeAsset | yes | What the maker wants in return (V1: must be `fungible`) |
| `expires_at` | u64 | no | Unix timestamp for expiration |
| `external_ref` | string | no | External reference (e.g., UI order ID) |
| `nonce` | u64 | yes | Account nonce |
| `signature` | hex string | yes | EIP-712 signature (65 bytes) |

---

### AcceptDeal

Accept an existing deal. For fungible-to-fungible deals, partial fills are supported via the `amount` field.

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "AcceptDeal",
    "from": "0x5678...ef01",
    "deal_id": 42,
    "amount": null,
    "nonce": 1,
    "signature": "0x..."
  }'
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `from` | hex string | yes | Taker's address |
| `deal_id` | u64 | yes | Deal to accept |
| `amount` | string/u128 | no | Partial fill amount (offer units). `null` = full fill. Ignored for escrowed offers. |
| `nonce` | u64 | yes | Account nonce |
| `signature` | hex string | yes | EIP-712 signature (65 bytes) |

---

### CancelDeal

Cancel a pending deal. Only the maker can cancel. Locked offer balance is refunded.

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "CancelDeal",
    "from": "0x1234...abcd",
    "deal_id": 42,
    "nonce": 3,
    "signature": "0x..."
  }'
```

---

### Withdraw

Withdraw funds from the Axync sequencer to an on-chain address.

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "Withdraw",
    "from": "0x1234...abcd",
    "asset_id": 0,
    "amount": "500000",
    "to": "0x1234...abcd",
    "chain_id": 1,
    "nonce": 4,
    "signature": "0x..."
  }'
```

---

### BuyNft

Buy an NFT listing directly (without going through the Deal flow). Used for simple fixed-price NFT purchases.

```bash
curl -X POST http://localhost:3000/api/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "kind": "BuyNft",
    "from": "0x5678...ef01",
    "listing_id": 7,
    "nonce": 2,
    "signature": "0x..."
  }'
```

---

## Proofs

### GET /api/v1/withdrawal-proof/:address/:asset_id/:amount/:chain_id

Get a merkle proof for withdrawing funds on-chain via AxyncVault.

```bash
curl http://localhost:3000/api/v1/withdrawal-proof/0x1234...abcd/0/500000/1
```

### GET /api/v1/nft-release-proof/:listing_id

Get a merkle proof for claiming an NFT/token from AxyncEscrow on-chain.

```bash
curl http://localhost:3000/api/v1/nft-release-proof/7
```

---

## NFT Listings

### GET /api/v1/nft-listings

List all NFT/token escrow listings tracked by the sequencer.

```bash
curl http://localhost:3000/api/v1/nft-listings
```

### GET /api/v1/nft-listing/:listing_id

Get details of a specific escrow listing.

```bash
curl http://localhost:3000/api/v1/nft-listing/7
```

---

## JSON-RPC

### POST /jsonrpc

Alternative submission endpoint using JSON-RPC 2.0.

```bash
curl -X POST http://localhost:3000/jsonrpc \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "submit_tx",
    "params": { "tx": "0x<bincode-hex>" },
    "id": 1
  }'
```

---

## Complete Deal Flow

### Scenario 1: Token-for-ETH Cross-Chain

```
1. Seller deposits tokens on Ethereum
   → Watcher detects Deposit event → submits Deposit tx to sequencer
   → Seller's balance: 1000 TOKEN (chain_id=1)

2. Seller creates deal
   POST /api/v1/transactions { kind: "CreateDeal", offer: {type: "fungible", asset_id: 1, amount: 1000, chain_id: 1}, consideration: {type: "fungible", asset_id: 0, amount: "600000000000000000", chain_id: 8453} }
   → Seller's 1000 TOKEN locked

3. Buyer deposits ETH on Base
   → Watcher detects Deposit event → submits Deposit tx
   → Buyer's balance: 0.6 ETH (chain_id=8453)

4. Buyer accepts deal
   POST /api/v1/transactions { kind: "AcceptDeal", deal_id: 42, amount: null }
   → Atomic swap: seller gets 0.6 ETH (Base), buyer gets 1000 TOKEN (Eth)

5. Both withdraw
   POST /api/v1/transactions { kind: "Withdraw", asset_id: 0, amount: "600000000000000000", chain_id: 8453 }
   → Wait for block proof → GET /api/v1/withdrawal-proof/... → call AxyncVault.withdraw() on-chain
```

### Scenario 2: NFT-for-Token Cross-Chain

```
1. Seller lists NFT in AxyncEscrow on Base (on-chain tx)
   → Watcher detects NftListed event → creates NftListing (id=7) in sequencer

2. Seller creates deal with escrowed offer
   POST /api/v1/transactions { kind: "CreateDeal", offer: {type: "escrowed", escrow_listing_id: 7}, consideration: {type: "fungible", asset_id: 0, amount: "500000000", chain_id: 1} }
   → Listing status: Reserved

3. Buyer deposits USDC on Ethereum
   → Balance: 500 USDC (chain_id=1)

4. Buyer accepts deal
   POST /api/v1/transactions { kind: "AcceptDeal", deal_id: 43 }
   → Seller gets 500 USDC (Eth), listing status: Sold, buyer set

5. Seller withdraws USDC
   → GET /api/v1/withdrawal-proof/... → AxyncVault.withdraw()

6. Buyer claims NFT
   → GET /api/v1/nft-release-proof/7 → AxyncEscrow.claim() on Base
```

### Scenario 3: ETH-for-Token Same Chain

```
Same as Scenario 1, but both offer and consideration use the same chain_id.
No cross-chain relaying needed — both withdrawals happen on the same chain.
```

---

## Error Responses

All errors follow this format:

```json
{
  "error": "ErrorCode",
  "message": "Human-readable description"
}
```

Common error codes:

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `InvalidAddress` | 400 | Malformed address (not valid hex or not 20 bytes) |
| `InvalidSignature` | 400 | Malformed signature (not valid hex or not 65 bytes) |
| `InvalidTxHash` | 400 | Malformed tx hash |
| `InvalidVisibility` | 400 | Must be "Public" or "Direct" |
| `AccountNotFound` | 404 | Account doesn't exist (use GET /api/v1/account/:address to auto-create) |
| `DealNotFound` | 404 | Deal ID not found |
| `BlockNotFound` | 404 | Block ID not found |
| `StorageNotAvailable` | 503 | Storage backend not configured |

---

## Rate Limiting

Default: 100 requests per 60 seconds per IP.

Configurable via environment variables:
- `RATE_LIMIT_MAX_REQUESTS` (default: 100)
- `RATE_LIMIT_WINDOW_SECONDS` (default: 60)

## CORS

CORS is fully permissive (`CorsLayer::permissive()`). All origins, methods, and headers are allowed.

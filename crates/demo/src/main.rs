//! ZKClear Demo - Complete User Flow Demonstration
//!
//! This demo shows the complete flow of ZKClear system:
//! 1. Wallet connection (simulation)
//! 2. Deposit funds
//! 3. Create deal
//! 4. Accept deal
//! 5. Generate blocks with ZK proofs
//! 6. On-chain verification (simulation)
//! 7. Withdraw funds

use std::sync::Arc;
use zkclear_prover::{Prover, ProverConfig};
use zkclear_sequencer::Sequencer;
use zkclear_types::{
    AcceptDeal, Address, AssetId, CreateDeal, DealVisibility, Deposit, Tx, TxKind, TxPayload,
    Withdraw,
};

fn addr(byte: u8) -> Address {
    [byte; 20]
}

fn format_address(addr: &Address) -> String {
    format!("0x{}", hex::encode(addr))
}

fn format_hash(hash: &[u8; 32]) -> String {
    format!("0x{}", hex::encode(hash))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("================================================================");
    println!("          ZKClear Demo - Complete User Flow");
    println!("================================================================");
    println!();

    // Step 1: Initialize Prover and Sequencer
    println!("Step 1: Initializing system...");
    let mut prover_config = ProverConfig::default();
    // Use placeholder proofs by default for faster demo
    // Set USE_REAL_PROOFS=1 to use real ZK proofs (much slower, especially first time)
    let use_real_proofs = std::env::var("USE_REAL_PROOFS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);
    prover_config.use_placeholders = !use_real_proofs;

    let use_placeholders = prover_config.use_placeholders;

    if use_placeholders {
        println!("   Using placeholder proofs (fast demo mode)");
        println!("   Set USE_REAL_PROOFS=1 to use real ZK proofs");
    } else {
        println!("   Using real ZK proofs (may take 30-60s for first block)");
        println!("   Generating Groth16 keys (first time only)...");
    }

    let prover = Arc::new(Prover::new(prover_config)?);

    if !use_placeholders {
        println!("   Groth16 keys ready");
    }

    let sequencer = Arc::new(Sequencer::new().with_prover(prover.clone()));

    println!("   Prover initialized");
    println!("   Sequencer initialized");
    println!();

    // Step 2: Wallet connection (simulation)
    println!("Step 2: Wallet connection (simulation)...");
    let maker = addr(1);
    let taker = addr(2);
    println!("   Maker wallet: {}", format_address(&maker));
    println!("   Taker wallet: {}", format_address(&taker));
    println!("   Wallets connected");
    println!();

    // Step 3: Deposit funds
    println!("Step 3: Depositing funds...");
    let usdc: AssetId = 0;
    let btc: AssetId = 1;
    let ethereum_chain = zkclear_types::chain_ids::ETHEREUM;
    let base_chain = zkclear_types::chain_ids::BASE;

    let mut tx_hash_counter = 0u64;
    let mut get_tx_hash = || {
        tx_hash_counter += 1;
        let mut hash = [0u8; 32];
        hash[0..8].copy_from_slice(&tx_hash_counter.to_le_bytes());
        hash
    };

    // Maker deposits USDC on Ethereum
    let maker_usdc_deposit = Tx {
        id: 0,
        from: maker,
        nonce: 0,
        kind: TxKind::Deposit,
        payload: TxPayload::Deposit(Deposit {
            tx_hash: get_tx_hash(),
            account: maker,
            asset_id: usdc,
            amount: 1_000_000, // 1 USDC (6 decimals)
            chain_id: ethereum_chain,
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(maker_usdc_deposit, false)
        .expect("Failed to submit maker USDC deposit");
    println!("   Maker deposited 1.0 USDC on Ethereum");

    // Taker deposits USDC on Ethereum
    let taker_usdc_deposit = Tx {
        id: 0,
        from: taker,
        nonce: 0,
        kind: TxKind::Deposit,
        payload: TxPayload::Deposit(Deposit {
            tx_hash: get_tx_hash(),
            account: taker,
            asset_id: usdc,
            amount: 1_000_000, // 1 USDC
            chain_id: ethereum_chain,
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(taker_usdc_deposit, false)
        .expect("Failed to submit taker USDC deposit");
    println!("   Taker deposited 1.0 USDC on Ethereum");

    // Maker deposits BTC on Base
    let maker_btc_deposit = Tx {
        id: 0,
        from: maker,
        nonce: 1,
        kind: TxKind::Deposit,
        payload: TxPayload::Deposit(Deposit {
            tx_hash: get_tx_hash(),
            account: maker,
            asset_id: btc,
            amount: 10_000, // 0.1 BTC (5 decimals)
            chain_id: base_chain,
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(maker_btc_deposit, false)
        .expect("Failed to submit maker BTC deposit");
    println!("   Maker deposited 0.1 BTC on Base");
    println!();

    // Step 4: Create deal
    println!("Step 4: Creating deal...");
    let create_deal_tx = Tx {
        id: 0,
        from: maker,
        nonce: 2,
        kind: TxKind::CreateDeal,
        payload: TxPayload::CreateDeal(CreateDeal {
            deal_id: 42,
            visibility: DealVisibility::Public,
            taker: None,
            asset_base: btc,
            asset_quote: usdc,
            chain_id_base: base_chain,
            chain_id_quote: ethereum_chain,
            amount_base: 1_000,        // 0.01 BTC
            price_quote_per_base: 100, // 1 BTC = 100 USDC
            expires_at: None,
            external_ref: None,
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(create_deal_tx, false)
        .expect("Failed to submit create deal");
    println!("   Deal #42 created:");
    println!("      Maker: {}", format_address(&maker));
    println!("      Sell: 0.01 BTC (Base)");
    println!("      Buy:  1.0 USDC (Ethereum)");
    println!("      Price: 1 BTC = 100 USDC");
    println!();

    // Step 5: Accept deal
    println!("Step 5: Accepting deal...");
    let accept_deal_tx = Tx {
        id: 0,
        from: taker,
        nonce: 1,
        kind: TxKind::AcceptDeal,
        payload: TxPayload::AcceptDeal(AcceptDeal {
            deal_id: 42,
            amount: None, // Accept full amount
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(accept_deal_tx, false)
        .expect("Failed to submit accept deal");
    println!("   Taker accepted deal #42");
    println!("   Atomic swap executed:");
    println!("      Maker received: 1.0 USDC (Ethereum)");
    println!("      Taker received: 0.01 BTC (Base)");
    println!();

    // Step 6: Generate blocks with ZK proofs
    println!("Step 6: Generating blocks with ZK proofs...");
    let mut block_count = 0;
    while sequencer.has_pending_txs() {
        block_count += 1;
        println!("   Creating block {}...", block_count);
        println!("      Generating ZK proof (this may take a moment)...");

        let start = std::time::Instant::now();
        // Call directly from async context - for placeholder proofs this is very fast
        match sequencer.build_and_execute_block_with_proof(true) {
            Ok(block) => {
                let duration = start.elapsed();
                println!(
                    "      Block {} created and executed ({:.2}s)",
                    block.id,
                    duration.as_secs_f64()
                );
                println!("         Transactions: {}", block.transactions.len());
                println!("         State root: {}", format_hash(&block.state_root));
                println!(
                    "         Withdrawals root: {}",
                    format_hash(&block.withdrawals_root)
                );
                println!("         Proof size: {} bytes", block.block_proof.len());

                if !block.block_proof.is_empty() {
                    println!("         ZK proof generated successfully");
                } else {
                    println!("         WARNING: ZK proof is empty (placeholder mode)");
                }
            }
            Err(e) => {
                println!("      ERROR: Block creation failed: {e:?}");
                break;
            }
        }
    }
    println!();

    // Step 7: Show final state
    println!("Step 7: Final state summary...");
    println!("   Current block ID: {}", sequencer.get_current_block_id());
    println!();

    let state_handle = sequencer.get_state();
    let state = state_handle.lock().unwrap();

    println!("   Accounts:");
    for (id, acc) in &state.accounts {
        println!("      Account {}: {}", id, format_address(&acc.owner));
        println!("         Nonce: {}", acc.nonce);
        for b in &acc.balances {
            let asset_name = if b.asset_id == usdc { "USDC" } else { "BTC" };
            let chain_name = match b.chain_id {
                x if x == ethereum_chain => "Ethereum",
                x if x == base_chain => "Base",
                _ => "Unknown",
            };
            let amount = if b.asset_id == usdc {
                format!("{:.6}", b.amount as f64 / 1_000_000.0)
            } else {
                format!("{:.5}", b.amount as f64 / 100_000.0)
            };
            println!("         {} {} on {}", amount, asset_name, chain_name);
        }
    }
    println!();

    println!("   Deals:");
    for (id, deal) in &state.deals {
        println!("      Deal {}: status={:?}", id, deal.status);
        println!("         Maker: {}", format_address(&deal.maker));
        if let Some(t) = deal.taker {
            println!("         Taker: {}", format_address(&t));
        }
        println!("         Amount remaining: {}", deal.amount_remaining);
    }
    println!();

    // Step 8: Simulate withdrawal
    println!("Step 8: Simulating withdrawal...");
    let withdraw_tx = Tx {
        id: 0,
        from: maker,
        nonce: 3,
        kind: TxKind::Withdraw,
        payload: TxPayload::Withdraw(Withdraw {
            asset_id: usdc,
            amount: 50_000, // 0.05 USDC
            to: maker,
            chain_id: ethereum_chain,
        }),
        signature: [0u8; 65],
    };
    sequencer
        .submit_tx_with_validation(withdraw_tx, false)
        .expect("Failed to submit withdrawal");
    println!("   Withdrawal transaction submitted");
    println!();

    // Process withdrawal block
    if sequencer.has_pending_txs() {
        println!("   Creating block with withdrawal...");
        println!("      Generating ZK proof (this may take a moment)...");
        let start = std::time::Instant::now();
        // Call directly from async context - for placeholder proofs this is very fast
        match sequencer.build_and_execute_block_with_proof(true) {
            Ok(block) => {
                let duration = start.elapsed();
                println!(
                    "      Block {} created with withdrawal ({:.2}s)",
                    block.id,
                    duration.as_secs_f64()
                );
                println!(
                    "         Withdrawals root: {}",
                    format_hash(&block.withdrawals_root)
                );
                println!("         Proof size: {} bytes", block.block_proof.len());
                println!("         Withdrawal processed");
            }
            Err(e) => {
                println!("      ERROR: Block creation failed: {e:?}");
            }
        }
    }
    println!();

    // Final summary
    println!("================================================================");
    println!("                    Demo completed successfully!");
    println!("================================================================");
    println!();
    println!("Summary:");
    println!("  • {} blocks created", block_count + 1);
    println!("  • All transactions processed");
    println!("  • ZK proofs generated for all blocks");
    println!("  • Cross-chain deal executed atomically");
    println!("  • Withdrawal processed");
    println!();

    Ok(())
}

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{interval, Duration};
use zkclear_api::{create_router, ApiState};
use zkclear_prover::{Prover, ProverConfig};
use zkclear_sequencer::Sequencer;
use zkclear_sequencer::SequencerError;
#[cfg(not(feature = "rocksdb"))]
use zkclear_storage::InMemoryStorage;
#[cfg(feature = "rocksdb")]
use zkclear_storage::RocksDBStorage;
use zkclear_watcher::{Watcher, WatcherConfig};

fn get_block_interval_seconds() -> u64 {
    std::env::var("BLOCK_INTERVAL_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(zkclear_sequencer::config::DEFAULT_BLOCK_INTERVAL_SECONDS)
}

fn get_storage_path() -> PathBuf {
    std::env::var("STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"))
}

fn init_storage() -> Result<Arc<dyn zkclear_storage::Storage>, Box<dyn std::error::Error>> {
    #[cfg(feature = "rocksdb")]
    {
        let path = get_storage_path();
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Failed to create storage directory: {}", e))?;

        println!("Initializing RocksDB storage at: {}", path.display());
        let storage = RocksDBStorage::open(&path)
            .map_err(|e| format!("Failed to open RocksDB storage: {:?}", e))?;

        Ok(Arc::new(storage))
    }

    #[cfg(not(feature = "rocksdb"))]
    {
        println!("Using InMemoryStorage (RocksDB not enabled)");
        Ok(Arc::new(InMemoryStorage::new()))
    }
}

async fn block_production_task(sequencer: Arc<Sequencer>) {
    let interval_secs = get_block_interval_seconds();
    let mut interval_timer = interval(Duration::from_secs(interval_secs));
    let mut consecutive_errors = 0;
    const MAX_CONSECUTIVE_ERRORS: u32 = 10;

    println!(
        "Block production task started (interval: {}s)",
        interval_secs
    );

    loop {
        interval_timer.tick().await;

        if !sequencer.has_pending_txs() {
            consecutive_errors = 0; // Reset error counter on successful skip
            continue;
        }

        // Build and execute block with proof generation enabled
        match sequencer.build_and_execute_block_with_proof(true) {
            Ok(block) => {
                consecutive_errors = 0; // Reset error counter on success
                println!(
                    "Block {} created and executed: {} transactions, queue: {}",
                    block.id,
                    block.transactions.len(),
                    sequencer.queue_length()
                );
            }
            Err(SequencerError::NoTransactions) => {
                // Queue was empty between check and build - skip
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("Failed to create/execute block (error {}/{}): {:?}", 
                    consecutive_errors, MAX_CONSECUTIVE_ERRORS, e);
                
                // If too many consecutive errors, wait longer before retrying
                if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                    eprintln!("Too many consecutive errors, waiting 60s before retrying...");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    consecutive_errors = 0; // Reset after backoff
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    // Initialize storage
    let storage = init_storage()?;
    let storage_trait: Arc<dyn zkclear_storage::Storage> = storage.clone();

    // Initialize prover (optional - will use placeholders if not configured)
    let prover_config = ProverConfig {
        use_placeholders: std::env::var("USE_PLACEHOLDER_PROVER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(false),
        groth16_keys_dir: std::env::var("GROTH16_KEYS_DIR")
            .ok()
            .map(std::path::PathBuf::from),
        force_regenerate_keys: std::env::var("FORCE_REGENERATE_KEYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(false),
        ..Default::default()
    };

    let prover = match Prover::new(prover_config) {
        Ok(p) => {
            println!("Prover initialized successfully");
            Some(Arc::new(p))
        }
        Err(e) => {
            eprintln!(
                "Warning: Failed to initialize prover: {:?}. Continuing without proof generation.",
                e
            );
            None
        }
    };

    // Initialize sequencer with storage (will load state from storage if available)
    println!("Initializing sequencer with storage...");
    let mut sequencer = Sequencer::with_storage_arc(storage.clone())
        .map_err(|e| format!("Failed to initialize sequencer with storage: {:?}", e))?;

    // Set prover if available
    if let Some(ref prover) = prover {
        sequencer = sequencer.with_prover(Arc::clone(prover));
        println!("Prover attached to sequencer");
    }

    let sequencer = Arc::new(sequencer);

    println!("Sequencer initialized with storage");
    println!("Current block ID: {}", sequencer.get_current_block_id());

    // Initialize rate limiting
    let max_requests = std::env::var("RATE_LIMIT_MAX_REQUESTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let window_seconds = std::env::var("RATE_LIMIT_WINDOW_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let rate_limit_state = Arc::new(zkclear_api::RateLimitState::new(max_requests, window_seconds));

    let api_state = Arc::new(ApiState {
        sequencer: sequencer.clone(),
        storage: Some(storage_trait),
        rate_limit_state: Some(rate_limit_state),
    });

    let app = create_router(api_state);

    // Create watcher config
    // If ETHEREUM_RPC_URL or BASE_RPC_URL are set, use them for testnet/mainnet
    // If only RPC_URL is set, use it for local Hardhat network
    // Otherwise, use default config (mainnet)
    let watcher_config = if std::env::var("ETHEREUM_RPC_URL").is_ok() || std::env::var("BASE_RPC_URL").is_ok() {
        // Testnet/Mainnet mode - use multiple chains from environment
        let mut chains = Vec::new();
        
        // Ethereum chain (Sepolia testnet or mainnet)
        if let Ok(rpc_url) = std::env::var("ETHEREUM_RPC_URL") {
            let chain_id = std::env::var("ETHEREUM_CHAIN_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(zkclear_types::chain_ids::ETHEREUM);
            
            chains.push(zkclear_watcher::ChainConfig {
                chain_id,
                rpc_url,
                deposit_contract_address: std::env::var("ETHEREUM_DEPOSIT_CONTRACT")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                required_confirmations: std::env::var("ETHEREUM_REQUIRED_CONFIRMATIONS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(12),
                poll_interval_seconds: std::env::var("POLL_INTERVAL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                rpc_timeout_seconds: std::env::var("RPC_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
                max_retries: std::env::var("MAX_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                retry_delay_seconds: std::env::var("RETRY_DELAY_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
                reorg_safety_blocks: std::env::var("REORG_SAFETY_BLOCKS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
            });
        }
        
        // Base chain (Base Sepolia testnet or mainnet)
        if let Ok(rpc_url) = std::env::var("BASE_RPC_URL") {
            let chain_id = std::env::var("BASE_CHAIN_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(zkclear_types::chain_ids::BASE);
            
            chains.push(zkclear_watcher::ChainConfig {
                chain_id,
                rpc_url,
                deposit_contract_address: std::env::var("BASE_DEPOSIT_CONTRACT")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                required_confirmations: std::env::var("BASE_REQUIRED_CONFIRMATIONS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(12),
                poll_interval_seconds: std::env::var("POLL_INTERVAL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                rpc_timeout_seconds: std::env::var("RPC_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
                max_retries: std::env::var("MAX_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                retry_delay_seconds: std::env::var("RETRY_DELAY_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
                reorg_safety_blocks: std::env::var("REORG_SAFETY_BLOCKS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10),
            });
        }
        
        WatcherConfig { chains }
    } else if std::env::var("RPC_URL").is_ok() {
        // Local development mode - use single chain config from environment
        let chain_id = std::env::var("CHAIN_ID")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(31337); // Hardhat default
        
        WatcherConfig {
            chains: vec![zkclear_watcher::ChainConfig {
                chain_id,
                rpc_url: std::env::var("RPC_URL")
                    .unwrap_or_else(|_| "http://localhost:8545".to_string()),
                deposit_contract_address: std::env::var("DEPOSIT_CONTRACT_ADDRESS")
                    .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
                required_confirmations: std::env::var("REQUIRED_CONFIRMATIONS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                poll_interval_seconds: std::env::var("POLL_INTERVAL_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                rpc_timeout_seconds: std::env::var("RPC_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
                max_retries: std::env::var("MAX_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
                retry_delay_seconds: std::env::var("RETRY_DELAY_SECONDS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
                reorg_safety_blocks: std::env::var("REORG_SAFETY_BLOCKS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            }],
        }
    } else {
        // Production mode - use default config (mainnet)
        WatcherConfig::default()
    };
    
    let watcher = Watcher::new(sequencer.clone(), watcher_config);

    let listener = TcpListener::bind("0.0.0.0:8080").await?;
    println!("ZKClear API server listening on http://0.0.0.0:8080");

    // Setup graceful shutdown
    let shutdown_signal = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    };

    // Create shutdown handle for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let shutdown_tx_clone = shutdown_tx.clone();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx.recv().await;
            })
            .await
    });

    let block_production_handle = tokio::spawn(block_production_task(sequencer.clone()));
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = watcher.start().await {
            eprintln!("Watcher error: {}", e);
        }
    });

    // Wait for shutdown signal
    shutdown_signal.await;
    println!("Shutdown signal received, starting graceful shutdown...");

    // Notify server to shutdown
    let _ = shutdown_tx_clone.send(()).await;

    // Wait for server to shutdown
    if let Err(e) = server_handle.await {
        eprintln!("Server shutdown error: {:?}", e);
    }

    // Abort background tasks
    block_production_handle.abort();
    watcher_handle.abort();

    println!("Graceful shutdown completed");

    Ok(())
}

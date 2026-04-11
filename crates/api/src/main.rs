use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{interval, Duration};
use axync_api::{create_router, ApiState};
use axync_prover::Prover;
use axync_sequencer::Sequencer;
use axync_sequencer::SequencerError;
#[cfg(not(feature = "rocksdb"))]
use axync_storage::InMemoryStorage;
#[cfg(feature = "rocksdb")]
use axync_storage::RocksDBStorage;
use axync_watcher::Watcher;

mod config;

fn init_storage() -> Result<Arc<dyn axync_storage::Storage>, Box<dyn std::error::Error>> {
    #[cfg(feature = "rocksdb")]
    {
        let path = config::storage_path();
        std::fs::create_dir_all(&path)?;
        println!("Storage: RocksDB at {}", path.display());
        let storage = RocksDBStorage::open(&path)
            .map_err(|e| format!("Failed to open RocksDB: {:?}", e))?;
        Ok(Arc::new(storage))
    }

    #[cfg(not(feature = "rocksdb"))]
    {
        println!("Storage: InMemory");
        Ok(Arc::new(InMemoryStorage::new()))
    }
}

async fn block_production_task(sequencer: Arc<Sequencer>) {
    let interval_secs = config::block_interval_sec();
    let mut timer = interval(Duration::from_secs(interval_secs));
    let mut consecutive_errors = 0u32;

    println!("Block production task started (interval: {}s)", interval_secs);

    loop {
        timer.tick().await;

        if !sequencer.has_pending_txs() {
            consecutive_errors = 0;
            continue;
        }

        match sequencer.build_and_execute_block_with_proof(true) {
            Ok(block) => {
                consecutive_errors = 0;
                println!(
                    "Block {} created and executed: {} transactions, queue: {}",
                    block.id, block.transactions.len(), sequencer.queue_length()
                );
            }
            Err(SequencerError::NoTransactions) => {
                consecutive_errors = 0;
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("Block production error ({}/10): {:?}", consecutive_errors, e);
                if consecutive_errors >= 10 {
                    eprintln!("Too many errors, backing off 60s...");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    consecutive_errors = 0;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env file (silently ignore if missing)
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Storage
    let storage = init_storage()?;
    let storage_trait: Arc<dyn axync_storage::Storage> = storage.clone();

    // Prover
    let prover = match Prover::new(config::prover_config()) {
        Ok(p) => {
            println!("Prover initialized");
            Some(Arc::new(p))
        }
        Err(e) => {
            eprintln!("Warning: Prover init failed: {:?}. Continuing without proofs.", e);
            None
        }
    };

    // Sequencer
    let mut sequencer = Sequencer::with_storage_arc(storage.clone())
        .map_err(|e| format!("Sequencer init failed: {:?}", e))?;

    if let Some(ref prover) = prover {
        sequencer = sequencer.with_prover(Arc::clone(prover));
    }

    let sequencer = Arc::new(sequencer);
    println!("Sequencer initialized with storage");
    println!("Current block ID: {}", sequencer.get_current_block_id());

    // Rate limiting
    let rate_limit_state = Arc::new(axync_api::RateLimitState::new(
        config::rate_limit_max_requests(),
        config::rate_limit_window_seconds(),
    ));

    // Marketplace readers
    let marketplace_rpc = config::marketplace_rpc();
    let vesting_reader = Arc::new(axync_api::vesting::VestingReader::new(marketplace_rpc.clone()));
    let nft_reader = Arc::new(axync_api::nft::NftReader::new(marketplace_rpc.clone()));

    let escrow_reader = config::escrow_contract().map(|addr| {
        Arc::new(axync_api::escrow::EscrowReader::new(marketplace_rpc.clone(), addr))
    });
    if escrow_reader.is_none() {
        println!("Warning: ESCROW_CONTRACT not set, listings endpoints disabled");
    }

    println!("Marketplace RPC: {}", marketplace_rpc);

    // API state
    let api_state = Arc::new(ApiState {
        sequencer: sequencer.clone(),
        storage: Some(storage_trait),
        rate_limit_state: Some(rate_limit_state),
        vesting_reader: Some(vesting_reader),
        escrow_reader,
        nft_reader: Some(nft_reader),
    });

    let app = create_router(api_state);

    // Watcher
    let watcher_config = config::watcher_config();
    let watcher = Watcher::new(sequencer.clone(), watcher_config);

    // Server
    let port = config::port();
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("Axync API server listening on http://0.0.0.0:{}", port);

    // Graceful shutdown
    let shutdown_signal = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
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

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let shutdown_tx_clone = shutdown_tx.clone();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown_rx.recv().await; })
            .await
    });

    let block_handle = tokio::spawn(block_production_task(sequencer.clone()));
    let watcher_handle = tokio::spawn(async move {
        if let Err(e) = watcher.start().await {
            eprintln!("Watcher error: {}", e);
        }
    });

    shutdown_signal.await;
    println!("Shutting down...");

    let _ = shutdown_tx_clone.send(()).await;
    if let Err(e) = server_handle.await {
        eprintln!("Server shutdown error: {:?}", e);
    }
    block_handle.abort();
    watcher_handle.abort();

    println!("Shutdown complete");
    Ok(())
}

mod chain_watcher;
mod config;
mod event_processor;
mod rpc_client;

pub use chain_watcher::ChainWatcher;
pub use config::{ChainConfig, WatcherConfig};
pub use event_processor::EventProcessor;
pub use rpc_client::RpcClient;

use std::sync::Arc;
use zkclear_sequencer::Sequencer;

pub struct Watcher {
    sequencer: Arc<Sequencer>,
    config: WatcherConfig,
}

impl Watcher {
    pub fn new(sequencer: Arc<Sequencer>, config: WatcherConfig) -> Self {
        Self { sequencer, config }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let mut handles = Vec::new();

        for chain_config in &self.config.chains {
            let watcher = ChainWatcher::new(chain_config.clone(), self.sequencer.clone())?;

            let handle = tokio::spawn(async move {
                if let Err(e) = watcher.watch().await {
                    eprintln!("Chain watcher error: {}", e);
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await?;
        }

        Ok(())
    }
}

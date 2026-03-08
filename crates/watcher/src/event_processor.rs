use std::sync::Arc;
use zkclear_sequencer::Sequencer;
use zkclear_types::{Address, AssetId, ChainId, Deposit, Tx, TxKind, TxPayload};

pub struct EventProcessor {
    sequencer: Arc<Sequencer>,
}

impl EventProcessor {
    pub fn new(sequencer: Arc<Sequencer>) -> Self {
        Self { sequencer }
    }

    pub fn process_deposit_event(
        &self,
        chain_id: ChainId,
        tx_hash: [u8; 32],
        account: Address,
        asset_id: AssetId,
        amount: u128,
    ) -> anyhow::Result<()> {
        let deposit = Deposit {
            tx_hash,
            account,
            asset_id,
            amount,
            chain_id,
        };

        let tx = Tx {
            id: 0,
            from: account,
            nonce: 0,
            kind: TxKind::Deposit,
            payload: TxPayload::Deposit(deposit),
            signature: [0u8; 65],
        };

        self.sequencer
            .submit_tx_with_validation(tx, false)
            .map_err(|e| anyhow::anyhow!("Failed to submit deposit tx: {:?}", e))?;

        Ok(())
    }
}

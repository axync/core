use std::sync::Arc;
use axync_sequencer::Sequencer;
use axync_types::{Address, AssetId, ChainId, Deposit, Tx, TxKind, TxPayload};

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

        // Look up the account's current nonce from sequencer state
        let nonce = {
            let state_handle = self.sequencer.get_state();
            let state_guard = state_handle.lock().unwrap();
            state_guard
                .get_account_by_address(account)
                .map(|a| a.nonce)
                .unwrap_or(0)
        };

        let tx = Tx {
            id: 0,
            from: account,
            nonce,
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

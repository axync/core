use std::sync::Arc;
use axync_sequencer::Sequencer;
use axync_types::{
    Address, AssetId, CancelNftListing, ChainId, Deposit, ListNft, Tx, TxKind, TxPayload,
};

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

    pub fn process_nft_listed_event(
        &self,
        nft_chain_id: ChainId,
        seller: Address,
        nft_contract: Address,
        token_id: u64,
        price: u128,
        payment_chain_id: u64,
        on_chain_listing_id: u64,
    ) -> anyhow::Result<()> {
        let list_nft = ListNft {
            seller,
            nft_contract,
            token_id,
            nft_chain_id,
            price,
            payment_chain_id,
            on_chain_listing_id,
        };

        let nonce = {
            let state_handle = self.sequencer.get_state();
            let state_guard = state_handle.lock().unwrap();
            state_guard
                .get_account_by_address(seller)
                .map(|a| a.nonce)
                .unwrap_or(0)
        };

        let tx = Tx {
            id: 0,
            from: seller,
            nonce,
            kind: TxKind::ListNft,
            payload: TxPayload::ListNft(list_nft),
            signature: [0u8; 65],
        };

        self.sequencer
            .submit_tx_with_validation(tx, false)
            .map_err(|e| anyhow::anyhow!("Failed to submit ListNft tx: {:?}", e))?;

        Ok(())
    }

    pub fn process_nft_cancelled_event(
        &self,
        on_chain_listing_id: u64,
    ) -> anyhow::Result<()> {
        // Find the sequencer listing by on_chain_listing_id
        let (listing_id, seller_addr, nonce) = {
            let state_handle = self.sequencer.get_state();
            let state_guard = state_handle.lock().unwrap();
            let mut found = None;
            for (id, listing) in &state_guard.nft_listings {
                if listing.on_chain_listing_id == on_chain_listing_id {
                    let n = state_guard
                        .get_account_by_address(listing.seller)
                        .map(|a| a.nonce)
                        .unwrap_or(0);
                    found = Some((*id, listing.seller, n));
                    break;
                }
            }
            found.ok_or_else(|| anyhow::anyhow!("Listing not found for on_chain_id {}", on_chain_listing_id))?
        };

        let cancel = CancelNftListing {
            listing_id,
            on_chain_listing_id,
        };

        let tx = Tx {
            id: 0,
            from: seller_addr,
            nonce,
            kind: TxKind::CancelNftListing,
            payload: TxPayload::CancelNftListing(cancel),
            signature: [0u8; 65],
        };

        self.sequencer
            .submit_tx_with_validation(tx, false)
            .map_err(|e| anyhow::anyhow!("Failed to submit CancelNftListing tx: {:?}", e))?;

        Ok(())
    }
}

use axync_state::State;
use axync_types::{
    AcceptDeal, Address, AssetId, Balance, BuyNft, CancelDeal, CancelNftListing, ChainId,
    CreateDeal, Deal, DealStatus, DealVisibility, Deposit, ListNft, NftListing, NftListingStatus,
    TradeAsset, Tx, TxPayload, Withdraw, ZERO_ADDRESS,
};

#[derive(Debug)]
pub enum StfError {
    UnsupportedTx,
    NotImplemented,
    BalanceTooLow,
    DealNotFound,
    DealAlreadyClosed,
    DealAlreadyExists,
    Unauthorized,
    Overflow,
    InvalidNonce,
    DealExpired,
    NftListingNotFound,
    NftListingNotActive,
}

pub fn apply_tx(state: &mut State, tx: &Tx, block_timestamp: u64) -> Result<(), StfError> {
    validate_nonce(state, tx.from, tx.nonce)?;

    let result = match &tx.payload {
        TxPayload::Deposit(p) => apply_deposit(state, p),
        TxPayload::Withdraw(p) => apply_withdraw(state, tx.from, p),
        TxPayload::CreateDeal(p) => apply_create_deal(state, tx.from, p, block_timestamp),
        TxPayload::AcceptDeal(p) => apply_accept_deal(state, tx.from, p, block_timestamp),
        TxPayload::CancelDeal(p) => apply_cancel_deal(state, tx.from, p),
        TxPayload::ListNft(p) => apply_list_nft(state, p, block_timestamp),
        TxPayload::BuyNft(p) => apply_buy_nft(state, tx.from, p),
        TxPayload::CancelNftListing(p) => apply_cancel_nft_listing(state, p),
    };

    if result.is_ok() {
        increment_nonce(state, tx.from);
    }

    result
}

fn apply_deposit(state: &mut State, payload: &Deposit) -> Result<(), StfError> {
    add_balance(
        state,
        payload.account,
        payload.asset_id,
        payload.amount,
        payload.chain_id,
    );
    Ok(())
}

fn apply_withdraw(state: &mut State, from: Address, payload: &Withdraw) -> Result<(), StfError> {
    sub_balance(
        state,
        from,
        payload.asset_id,
        payload.amount,
        payload.chain_id,
    )
}

pub fn apply_block(state: &mut State, txs: &[Tx], block_timestamp: u64) -> Result<(), StfError> {
    for tx in txs {
        apply_tx(state, tx, block_timestamp)?;
    }
    Ok(())
}

fn apply_create_deal(
    state: &mut State,
    maker: Address,
    payload: &CreateDeal,
    block_timestamp: u64,
) -> Result<(), StfError> {
    if state.get_deal(payload.deal_id).is_some() {
        return Err(StfError::DealAlreadyExists);
    }

    // V1: consideration must be Fungible
    match &payload.consideration {
        TradeAsset::Fungible { .. } => {}
        TradeAsset::Escrowed { .. } => return Err(StfError::NotImplemented),
    }

    // Lock the maker's offered asset
    match &payload.offer {
        TradeAsset::Fungible { asset_id, amount, chain_id } => {
            sub_balance(state, maker, *asset_id, *amount, *chain_id)?;
        }
        TradeAsset::Escrowed { escrow_listing_id } => {
            let listing = state
                .get_nft_listing_mut(*escrow_listing_id)
                .ok_or(StfError::NftListingNotFound)?;
            if listing.status != NftListingStatus::Active {
                return Err(StfError::NftListingNotActive);
            }
            if listing.seller != maker {
                return Err(StfError::Unauthorized);
            }
            listing.status = NftListingStatus::Reserved;
        }
    }

    let offer_chain = offer_chain_id(&payload.offer, state);
    let cons_chain = match &payload.consideration {
        TradeAsset::Fungible { chain_id, .. } => Some(*chain_id),
        TradeAsset::Escrowed { .. } => None,
    };
    let is_cross_chain = match (offer_chain, cons_chain) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    };

    let expires_at = payload.expires_at.map(|exp| {
        use axync_types::deal;
        let max_expiry = block_timestamp + deal::MAX_DEAL_DURATION_SECONDS;
        exp.min(max_expiry)
    });

    let deal = Deal {
        id: payload.deal_id,
        maker,
        taker: payload.taker,
        visibility: payload.visibility,
        offer: payload.offer.clone(),
        consideration: payload.consideration.clone(),
        amount_filled: 0,
        status: DealStatus::Pending,
        created_at: block_timestamp,
        expires_at,
        external_ref: payload.external_ref.clone(),
        is_cross_chain,
    };

    state.upsert_deal(deal);
    Ok(())
}

fn offer_chain_id(offer: &TradeAsset, state: &State) -> Option<ChainId> {
    match offer {
        TradeAsset::Fungible { chain_id, .. } => Some(*chain_id),
        TradeAsset::Escrowed { escrow_listing_id } => {
            state.get_nft_listing(*escrow_listing_id).map(|l| l.nft_chain_id)
        }
    }
}

fn apply_accept_deal(
    state: &mut State,
    taker: Address,
    payload: &AcceptDeal,
    block_timestamp: u64,
) -> Result<(), StfError> {
    // Extract deal data (borrow state immutably first)
    let (maker_addr, offer, consideration, _visibility, _expires_at, _expected_taker, amount_filled) = {
        let deal = state
            .get_deal(payload.deal_id)
            .ok_or(StfError::DealNotFound)?;

        if deal.status != DealStatus::Pending {
            return Err(StfError::DealAlreadyClosed);
        }

        if let Some(exp) = deal.expires_at {
            if exp > 0 && exp < block_timestamp {
                return Err(StfError::DealExpired);
            }
        }

        match deal.visibility {
            DealVisibility::Public => {}
            DealVisibility::Direct => {
                if let Some(expected) = deal.taker {
                    if expected != taker {
                        return Err(StfError::Unauthorized);
                    }
                } else {
                    return Err(StfError::Unauthorized);
                }
            }
        }

        if deal.maker == taker {
            return Err(StfError::Unauthorized);
        }

        (
            deal.maker,
            deal.offer.clone(),
            deal.consideration.clone(),
            deal.visibility,
            deal.expires_at,
            deal.taker,
            deal.amount_filled,
        )
    };

    match (&offer, &consideration) {
        // Fungible ↔ Fungible: supports partial fills
        (
            TradeAsset::Fungible { asset_id: offer_aid, amount: offer_amt, chain_id: offer_cid },
            TradeAsset::Fungible { asset_id: cons_aid, amount: cons_amt, chain_id: cons_cid },
        ) => {
            let remaining = offer_amt - amount_filled;
            let fill = payload.amount.unwrap_or(remaining);
            if fill == 0 || fill > remaining {
                return Err(StfError::BalanceTooLow);
            }

            // Scale consideration proportionally: (cons_amt * fill) / offer_amt
            // Short-circuit for full fill (most common case, avoids overflow)
            let consideration_fill = if fill == remaining && remaining == *offer_amt {
                *cons_amt
            } else {
                mul_div_u128(*cons_amt, fill, *offer_amt).ok_or(StfError::Overflow)?
            };

            // Taker pays consideration
            sub_balance(state, taker, *cons_aid, consideration_fill, *cons_cid)?;
            // Maker receives consideration
            add_balance(state, maker_addr, *cons_aid, consideration_fill, *cons_cid);
            // Taker receives offer (was already locked from maker at creation)
            add_balance(state, taker, *offer_aid, fill, *offer_cid);

            let deal = state.get_deal_mut(payload.deal_id).ok_or(StfError::DealNotFound)?;
            deal.amount_filled += fill;
            if deal.amount_filled >= *offer_amt {
                deal.status = DealStatus::Settled;
            }
        }

        // Escrowed ↔ Fungible: NFT/ERC20 from escrow for fungible payment. Full fill only.
        (
            TradeAsset::Escrowed { escrow_listing_id },
            TradeAsset::Fungible { asset_id: cons_aid, amount: cons_amt, chain_id: cons_cid },
        ) => {
            // Taker pays full consideration
            sub_balance(state, taker, *cons_aid, *cons_amt, *cons_cid)?;
            // Maker receives payment
            add_balance(state, maker_addr, *cons_aid, *cons_amt, *cons_cid);

            // Mark the escrow listing as sold with taker as buyer
            let listing = state
                .get_nft_listing_mut(*escrow_listing_id)
                .ok_or(StfError::NftListingNotFound)?;
            listing.status = NftListingStatus::Sold;
            listing.buyer = taker;

            let deal = state.get_deal_mut(payload.deal_id).ok_or(StfError::DealNotFound)?;
            deal.amount_filled = 1; // sentinel for escrowed deals
            deal.status = DealStatus::Settled;
        }

        _ => return Err(StfError::NotImplemented),
    }

    Ok(())
}

fn apply_cancel_deal(
    state: &mut State,
    caller: Address,
    payload: &CancelDeal,
) -> Result<(), StfError> {
    // Read deal data first
    let (maker, offer, amount_filled) = {
        let deal = state
            .get_deal(payload.deal_id)
            .ok_or(StfError::DealNotFound)?;

        if deal.status != DealStatus::Pending {
            return Err(StfError::DealAlreadyClosed);
        }

        if deal.maker != caller {
            return Err(StfError::Unauthorized);
        }

        (deal.maker, deal.offer.clone(), deal.amount_filled)
    };

    // Refund the locked offer
    match &offer {
        TradeAsset::Fungible { asset_id, amount, chain_id } => {
            let refund = amount - amount_filled;
            if refund > 0 {
                add_balance(state, maker, *asset_id, refund, *chain_id);
            }
        }
        TradeAsset::Escrowed { escrow_listing_id } => {
            // Un-reserve the listing
            if let Some(listing) = state.get_nft_listing_mut(*escrow_listing_id) {
                listing.status = NftListingStatus::Active;
            }
        }
    }

    let deal = state.get_deal_mut(payload.deal_id).ok_or(StfError::DealNotFound)?;
    deal.status = DealStatus::Cancelled;

    Ok(())
}

// ── NFT Escrow STF ──

/// Created by watcher from NftListed on-chain event
fn apply_list_nft(
    state: &mut State,
    payload: &ListNft,
    block_timestamp: u64,
) -> Result<(), StfError> {
    let id = state.next_nft_listing_id;
    state.next_nft_listing_id += 1;

    let listing = NftListing {
        id,
        seller: payload.seller,
        asset_type: payload.asset_type,
        nft_contract: payload.nft_contract,
        token_id: payload.token_id,
        amount: payload.amount,
        nft_chain_id: payload.nft_chain_id,
        price: payload.price,
        payment_chain_id: payload.payment_chain_id,
        status: NftListingStatus::Active,
        buyer: ZERO_ADDRESS,
        created_at: block_timestamp,
        on_chain_listing_id: payload.on_chain_listing_id,
    };

    state.upsert_nft_listing(listing);
    Ok(())
}

/// Submitted by buyer (user-signed). Deducts buyer balance, credits seller.
fn apply_buy_nft(
    state: &mut State,
    buyer: Address,
    payload: &BuyNft,
) -> Result<(), StfError> {
    // Read listing data first
    let (seller, price, payment_chain_id) = {
        let listing = state
            .get_nft_listing(payload.listing_id)
            .ok_or(StfError::NftListingNotFound)?;

        if listing.status != NftListingStatus::Active {
            return Err(StfError::NftListingNotActive);
        }

        if listing.seller == buyer {
            return Err(StfError::Unauthorized);
        }

        (listing.seller, listing.price, listing.payment_chain_id)
    };

    // ETH = asset_id 0
    let asset_id: u16 = 0;

    // Check and deduct buyer balance
    ensure_balance(state, buyer, asset_id, price, payment_chain_id)?;
    sub_balance(state, buyer, asset_id, price, payment_chain_id)?;

    // Credit seller (fee is taken on-chain when seller withdraws, not here)
    add_balance(state, seller, asset_id, price, payment_chain_id);

    // Mark listing as sold
    let listing = state
        .get_nft_listing_mut(payload.listing_id)
        .ok_or(StfError::NftListingNotFound)?;
    listing.status = NftListingStatus::Sold;
    listing.buyer = buyer;

    Ok(())
}

/// Created by watcher from NftCancelled on-chain event
fn apply_cancel_nft_listing(
    state: &mut State,
    payload: &CancelNftListing,
) -> Result<(), StfError> {
    let listing = state
        .get_nft_listing_mut(payload.listing_id)
        .ok_or(StfError::NftListingNotFound)?;

    if listing.status != NftListingStatus::Active {
        return Err(StfError::NftListingNotActive);
    }

    listing.status = NftListingStatus::Cancelled;
    Ok(())
}

fn add_balance(
    state: &mut State,
    owner: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
) {
    let account = state.get_or_create_account_by_owner(owner);

    for b in &mut account.balances {
        if b.asset_id == asset_id && b.chain_id == chain_id {
            b.amount = b.amount.saturating_add(amount);
            return;
        }
    }

    account.balances.push(Balance {
        asset_id,
        amount,
        chain_id,
    });
}

fn sub_balance(
    state: &mut State,
    owner: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
) -> Result<(), StfError> {
    let account = state.get_or_create_account_by_owner(owner);

    for b in &mut account.balances {
        if b.asset_id == asset_id && b.chain_id == chain_id {
            if b.amount < amount {
                return Err(StfError::BalanceTooLow);
            }
            b.amount -= amount;
            return Ok(());
        }
    }

    Err(StfError::BalanceTooLow)
}

fn ensure_balance(
    state: &mut State,
    owner: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
) -> Result<(), StfError> {
    let account = state.get_or_create_account_by_owner(owner);

    for b in &account.balances {
        if b.asset_id == asset_id && b.chain_id == chain_id {
            if b.amount < amount {
                return Err(StfError::BalanceTooLow);
            }
            return Ok(());
        }
    }

    Err(StfError::BalanceTooLow)
}

fn validate_nonce(state: &mut State, owner: Address, tx_nonce: u64) -> Result<(), StfError> {
    let account = state.get_or_create_account_by_owner(owner);
    let expected_nonce = account.nonce;

    if tx_nonce != expected_nonce {
        return Err(StfError::InvalidNonce);
    }

    Ok(())
}

fn increment_nonce(state: &mut State, owner: Address) {
    let account = state.get_or_create_account_by_owner(owner);
    account.nonce += 1;
}

/// Safe (a * b) / c computation that avoids u128 overflow in the intermediate product.
/// With 18-decimal tokens, a*b can easily exceed u128::MAX (3.4e38).
fn mul_div_u128(a: u128, b: u128, c: u128) -> Option<u128> {
    if c == 0 {
        return None;
    }
    // Fast path: no overflow
    if let Some(product) = a.checked_mul(b) {
        return Some(product / c);
    }
    // Overflow path: decompose a = (a/c)*c + (a%c), then:
    //   a*b/c = (a/c)*b + (a%c)*b/c
    let q = a / c;
    let r = a % c; // r < c
    let term1 = q.checked_mul(b)?;
    match r.checked_mul(b) {
        Some(rb) => Some(term1 + rb / c),
        None => {
            // Both r*b and a*b overflow. Decompose b similarly:
            //   r*b/c = (b/c)*r + (b%c)*r/c
            let q2 = b / c;
            let r2 = b % c; // r2 < c
            // r < c and r2 < c, so r*r2 < c^2. For c up to ~1.8e19 (u64 range), c^2 < u128::MAX.
            let term2 = q2.checked_mul(r)?;
            let term3 = r2.checked_mul(r)? / c;
            Some(term1 + term2 + term3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axync_types::{TradeAsset, Tx, TxKind, TxPayload};

    fn dummy_address(byte: u8) -> Address {
        [byte; 20]
    }

    fn default_chain_id() -> ChainId {
        axync_types::chain_ids::ETHEREUM
    }

    fn dummy_tx(from: Address, nonce: u64, payload: TxPayload) -> Tx {
        Tx {
            id: 0,
            from,
            nonce,
            kind: match &payload {
                TxPayload::Deposit(_) => TxKind::Deposit,
                TxPayload::Withdraw(_) => TxKind::Withdraw,
                TxPayload::CreateDeal(_) => TxKind::CreateDeal,
                TxPayload::AcceptDeal(_) => TxKind::AcceptDeal,
                TxPayload::CancelDeal(_) => TxKind::CancelDeal,
                TxPayload::ListNft(_) => TxKind::ListNft,
                TxPayload::BuyNft(_) => TxKind::BuyNft,
                TxPayload::CancelNftListing(_) => TxKind::CancelNftListing,
            },
            payload,
            signature: [0u8; 65],
        }
    }

    #[test]
    fn test_deposit() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        let tx = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: addr,
                asset_id: 0,
                amount: 1000,
                chain_id: default_chain_id(),
            }),
        );

        apply_tx(&mut state, &tx, block_timestamp).unwrap();

        let account = state.get_account_by_address(addr).unwrap();
        assert_eq!(account.balances.len(), 1);
        assert_eq!(account.balances[0].asset_id, 0);
        assert_eq!(account.balances[0].chain_id, default_chain_id());
        assert_eq!(account.balances[0].amount, 1000);
        assert_eq!(account.nonce, 1);
    }

    #[test]
    fn test_deposit_multiple_assets() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        let tx1 = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: addr,
                asset_id: 0,
                amount: 1000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &tx1, block_timestamp).unwrap();

        let tx2 = dummy_tx(
            addr,
            1,
            TxPayload::Deposit(Deposit {
                tx_hash: [1u8; 32],
                account: addr,
                asset_id: 1,
                amount: 500,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &tx2, block_timestamp).unwrap();

        let account = state.get_account_by_address(addr).unwrap();
        assert_eq!(account.balances.len(), 2);
        assert_eq!(account.nonce, 2);
    }

    #[test]
    fn test_withdraw() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        let deposit_tx = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: addr,
                asset_id: 0,
                amount: 1000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &deposit_tx, block_timestamp).unwrap();

        let withdraw_tx = dummy_tx(
            addr,
            1,
            TxPayload::Withdraw(Withdraw {
                asset_id: 0,
                amount: 300,
                to: addr,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &withdraw_tx, block_timestamp).unwrap();

        let account = state.get_account_by_address(addr).unwrap();
        assert_eq!(account.balances[0].amount, 700);
    }

    #[test]
    fn test_withdraw_insufficient_balance() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        let deposit_tx = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: addr,
                asset_id: 0,
                amount: 100,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &deposit_tx, block_timestamp).unwrap();

        let withdraw_tx = dummy_tx(
            addr,
            1,
            TxPayload::Withdraw(Withdraw {
                asset_id: 0,
                amount: 200,
                to: addr,
                chain_id: default_chain_id(),
            }),
        );

        assert!(matches!(
            apply_tx(&mut state, &withdraw_tx, block_timestamp),
            Err(StfError::BalanceTooLow)
        ));
    }

    #[test]
    fn test_create_deal() {
        let mut state = State::new();
        let maker = dummy_address(1);
        let block_timestamp = 1000;

        let deposit_tx = dummy_tx(
            maker,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: maker,
                asset_id: 0,
                amount: 10000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &deposit_tx, block_timestamp).unwrap();

        let create_deal_tx = dummy_tx(
            maker,
            1,
            TxPayload::CreateDeal(CreateDeal {
                deal_id: 42,
                visibility: DealVisibility::Public,
                taker: None,
                offer: TradeAsset::Fungible {
                    asset_id: 0,
                    amount: 1000,
                    chain_id: default_chain_id(),
                },
                consideration: TradeAsset::Fungible {
                    asset_id: 1,
                    amount: 100000,
                    chain_id: default_chain_id(),
                },
                expires_at: None,
                external_ref: None,
            }),
        );
        apply_tx(&mut state, &create_deal_tx, block_timestamp).unwrap();

        let deal = state.get_deal(42).unwrap();
        assert_eq!(deal.maker, maker);
        assert_eq!(deal.amount_filled, 0);
        assert_eq!(deal.status, DealStatus::Pending);

        // Maker's balance should be locked (10000 - 1000 = 9000)
        let maker_account = state.get_account_by_address(maker).unwrap();
        let balance = maker_account.balances.iter().find(|b| b.asset_id == 0).unwrap();
        assert_eq!(balance.amount, 9000);
    }

    #[test]
    fn test_accept_deal_fungible() {
        let mut state = State::new();
        let maker = dummy_address(1);
        let taker = dummy_address(2);
        let block_timestamp = 1000;

        // Maker deposits 10000 of asset 0
        let maker_deposit = dummy_tx(
            maker,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: maker,
                asset_id: 0,
                amount: 10000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &maker_deposit, block_timestamp).unwrap();

        // Taker deposits 100000 of asset 1
        let taker_deposit = dummy_tx(
            taker,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [1u8; 32],
                account: taker,
                asset_id: 1,
                amount: 100000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &taker_deposit, block_timestamp).unwrap();

        // Maker offers 1000 of asset 0 for 100000 of asset 1
        let create_deal = dummy_tx(
            maker,
            1,
            TxPayload::CreateDeal(CreateDeal {
                deal_id: 42,
                visibility: DealVisibility::Public,
                taker: None,
                offer: TradeAsset::Fungible {
                    asset_id: 0,
                    amount: 1000,
                    chain_id: default_chain_id(),
                },
                consideration: TradeAsset::Fungible {
                    asset_id: 1,
                    amount: 100000,
                    chain_id: default_chain_id(),
                },
                expires_at: None,
                external_ref: None,
            }),
        );
        apply_tx(&mut state, &create_deal, block_timestamp).unwrap();

        // Taker accepts full deal
        let accept_deal = dummy_tx(
            taker,
            1,
            TxPayload::AcceptDeal(AcceptDeal {
                deal_id: 42,
                amount: None,
            }),
        );
        apply_tx(&mut state, &accept_deal, block_timestamp).unwrap();

        let deal = state.get_deal(42).unwrap();
        assert_eq!(deal.status, DealStatus::Settled);

        let maker_account = state.get_account_by_address(maker).unwrap();
        let taker_account = state.get_account_by_address(taker).unwrap();

        // Maker: started with 10000, locked 1000, received 100000 of asset 1
        let maker_asset0 = maker_account.balances.iter().find(|b| b.asset_id == 0).map(|b| b.amount).unwrap_or(0);
        assert_eq!(maker_asset0, 9000); // 10000 - 1000 locked
        let maker_asset1 = maker_account.balances.iter().find(|b| b.asset_id == 1).map(|b| b.amount).unwrap_or(0);
        assert_eq!(maker_asset1, 100000);

        // Taker: received 1000 of asset 0, paid 100000 of asset 1
        let taker_asset0 = taker_account.balances.iter().find(|b| b.asset_id == 0).map(|b| b.amount).unwrap_or(0);
        assert_eq!(taker_asset0, 1000);
        let taker_asset1 = taker_account.balances.iter().find(|b| b.asset_id == 1).map(|b| b.amount).unwrap_or(0);
        assert_eq!(taker_asset1, 0);
    }

    #[test]
    fn test_cancel_deal_refund() {
        let mut state = State::new();
        let maker = dummy_address(1);
        let block_timestamp = 1000;

        let deposit_tx = dummy_tx(
            maker,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: maker,
                asset_id: 0,
                amount: 5000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &deposit_tx, block_timestamp).unwrap();

        let create_deal = dummy_tx(
            maker,
            1,
            TxPayload::CreateDeal(CreateDeal {
                deal_id: 1,
                visibility: DealVisibility::Public,
                taker: None,
                offer: TradeAsset::Fungible {
                    asset_id: 0,
                    amount: 3000,
                    chain_id: default_chain_id(),
                },
                consideration: TradeAsset::Fungible {
                    asset_id: 1,
                    amount: 9000,
                    chain_id: default_chain_id(),
                },
                expires_at: None,
                external_ref: None,
            }),
        );
        apply_tx(&mut state, &create_deal, block_timestamp).unwrap();

        // Balance after lock: 5000 - 3000 = 2000
        let acct = state.get_account_by_address(maker).unwrap();
        assert_eq!(acct.balances[0].amount, 2000);

        // Cancel — should refund
        let cancel = dummy_tx(
            maker,
            2,
            TxPayload::CancelDeal(CancelDeal { deal_id: 1 }),
        );
        apply_tx(&mut state, &cancel, block_timestamp).unwrap();

        let deal = state.get_deal(1).unwrap();
        assert_eq!(deal.status, DealStatus::Cancelled);

        // Balance restored: 2000 + 3000 = 5000
        let acct = state.get_account_by_address(maker).unwrap();
        assert_eq!(acct.balances[0].amount, 5000);
    }

    #[test]
    fn test_invalid_nonce() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        let tx1 = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: addr,
                asset_id: 0,
                amount: 1000,
                chain_id: default_chain_id(),
            }),
        );
        apply_tx(&mut state, &tx1, block_timestamp).unwrap();

        let tx2 = dummy_tx(
            addr,
            0,
            TxPayload::Deposit(Deposit {
                tx_hash: [1u8; 32],
                account: addr,
                asset_id: 0,
                amount: 1000,
                chain_id: default_chain_id(),
            }),
        );

        assert!(matches!(
            apply_tx(&mut state, &tx2, block_timestamp),
            Err(StfError::InvalidNonce)
        ));
    }

    #[test]
    fn test_nonce_increment() {
        let mut state = State::new();
        let addr = dummy_address(1);
        let block_timestamp = 1000;

        for i in 0..5 {
            let tx = dummy_tx(
                addr,
                i,
                TxPayload::Deposit(Deposit {
                    tx_hash: [i as u8; 32],
                    account: addr,
                    asset_id: 0,
                    amount: 100,
                    chain_id: default_chain_id(),
                }),
            );
            apply_tx(&mut state, &tx, block_timestamp).unwrap();
        }

        let account = state.get_account_by_address(addr).unwrap();
        assert_eq!(account.nonce, 5);
    }

    #[test]
    fn test_accept_deal_large_amounts_no_overflow() {
        // Reproduces the u128 overflow bug:
        // cons_amt(6e17) * fill(1e21) = 6e38 > u128::MAX(3.4e38)
        let mut state = State::new();
        let seller = dummy_address(1);
        let buyer = dummy_address(2);
        let ts = 1000;

        let token_amount: u128 = 1_000_000_000_000_000_000_000; // 1000e18 tokens
        let eth_amount: u128 = 600_000_000_000_000_000; // 0.6e18 ETH

        // Seller deposits tokens
        apply_tx(&mut state, &dummy_tx(seller, 0, TxPayload::Deposit(Deposit {
            tx_hash: [0u8; 32], account: seller, asset_id: 1,
            amount: token_amount, chain_id: default_chain_id(),
        })), ts).unwrap();

        // Buyer deposits ETH
        apply_tx(&mut state, &dummy_tx(buyer, 0, TxPayload::Deposit(Deposit {
            tx_hash: [1u8; 32], account: buyer, asset_id: 0,
            amount: eth_amount, chain_id: default_chain_id(),
        })), ts).unwrap();

        // Seller creates deal: 1000e18 tokens for 0.6e18 ETH
        apply_tx(&mut state, &dummy_tx(seller, 1, TxPayload::CreateDeal(CreateDeal {
            deal_id: 99,
            visibility: DealVisibility::Public,
            taker: None,
            offer: TradeAsset::Fungible { asset_id: 1, amount: token_amount, chain_id: default_chain_id() },
            consideration: TradeAsset::Fungible { asset_id: 0, amount: eth_amount, chain_id: default_chain_id() },
            expires_at: None,
            external_ref: None,
        })), ts).unwrap();

        // Buyer accepts — this used to overflow in cons_amt.checked_mul(fill)
        apply_tx(&mut state, &dummy_tx(buyer, 1, TxPayload::AcceptDeal(AcceptDeal {
            deal_id: 99, amount: None,
        })), ts).unwrap();

        let deal = state.get_deal(99).unwrap();
        assert_eq!(deal.status, DealStatus::Settled);

        let seller_eth = state.get_account_by_address(seller).unwrap()
            .balances.iter().find(|b| b.asset_id == 0).map(|b| b.amount).unwrap_or(0);
        assert_eq!(seller_eth, eth_amount);

        let buyer_tokens = state.get_account_by_address(buyer).unwrap()
            .balances.iter().find(|b| b.asset_id == 1).map(|b| b.amount).unwrap_or(0);
        assert_eq!(buyer_tokens, token_amount);
    }

    #[test]
    fn test_accept_deal_partial_fill_large_amounts() {
        // Partial fill with 18-decimal amounts that would overflow naive multiplication
        let mut state = State::new();
        let seller = dummy_address(1);
        let buyer = dummy_address(2);
        let ts = 1000;

        let offer_amount: u128 = 1_000_000_000_000_000_000_000; // 1000e18
        let cons_amount: u128 = 600_000_000_000_000_000; // 0.6e18
        let fill_amount: u128 = 500_000_000_000_000_000_000; // 500e18 (half)

        // Deposits
        apply_tx(&mut state, &dummy_tx(seller, 0, TxPayload::Deposit(Deposit {
            tx_hash: [0u8; 32], account: seller, asset_id: 1,
            amount: offer_amount, chain_id: default_chain_id(),
        })), ts).unwrap();
        apply_tx(&mut state, &dummy_tx(buyer, 0, TxPayload::Deposit(Deposit {
            tx_hash: [1u8; 32], account: buyer, asset_id: 0,
            amount: cons_amount, chain_id: default_chain_id(),
        })), ts).unwrap();

        // Create deal
        apply_tx(&mut state, &dummy_tx(seller, 1, TxPayload::CreateDeal(CreateDeal {
            deal_id: 100,
            visibility: DealVisibility::Public,
            taker: None,
            offer: TradeAsset::Fungible { asset_id: 1, amount: offer_amount, chain_id: default_chain_id() },
            consideration: TradeAsset::Fungible { asset_id: 0, amount: cons_amount, chain_id: default_chain_id() },
            expires_at: None,
            external_ref: None,
        })), ts).unwrap();

        // Buyer accepts HALF — fill 500e18 out of 1000e18
        apply_tx(&mut state, &dummy_tx(buyer, 1, TxPayload::AcceptDeal(AcceptDeal {
            deal_id: 100, amount: Some(fill_amount),
        })), ts).unwrap();

        let deal = state.get_deal(100).unwrap();
        assert_eq!(deal.amount_filled, fill_amount);
        assert_eq!(deal.status, DealStatus::Pending); // partially filled

        // Buyer paid half the consideration: 0.3e18
        let expected_cons_fill = cons_amount * fill_amount / offer_amount; // 300000000000000000
        let buyer_eth = state.get_account_by_address(buyer).unwrap()
            .balances.iter().find(|b| b.asset_id == 0).map(|b| b.amount).unwrap_or(0);
        assert_eq!(buyer_eth, cons_amount - expected_cons_fill);

        // Buyer got 500e18 tokens
        let buyer_tokens = state.get_account_by_address(buyer).unwrap()
            .balances.iter().find(|b| b.asset_id == 1).map(|b| b.amount).unwrap_or(0);
        assert_eq!(buyer_tokens, fill_amount);
    }

    #[test]
    fn test_mul_div_u128() {
        // Basic case: no overflow
        assert_eq!(mul_div_u128(100, 200, 50), Some(400));

        // Case that triggers overflow: 6e17 * 1e21 = 6e38 > u128::MAX
        let result = mul_div_u128(600_000_000_000_000_000, 1_000_000_000_000_000_000_000, 1_000_000_000_000_000_000_000);
        assert_eq!(result, Some(600_000_000_000_000_000));

        // Partial fill overflow: 6e17 * 5e20 / 1e21 = 3e17
        let result = mul_div_u128(600_000_000_000_000_000, 500_000_000_000_000_000_000, 1_000_000_000_000_000_000_000);
        assert_eq!(result, Some(300_000_000_000_000_000));

        // Division by zero
        assert_eq!(mul_div_u128(100, 200, 0), None);

        // Edge: a=0
        assert_eq!(mul_div_u128(0, u128::MAX, 1), Some(0));
    }
}

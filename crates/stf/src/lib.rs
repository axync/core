use zkclear_state::State;
use zkclear_types::{
    AcceptDeal, Address, AssetId, Balance, CancelDeal, ChainId, CreateDeal, Deal, DealStatus,
    DealVisibility, Deposit, Tx, TxPayload, Withdraw,
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
}

pub fn apply_tx(state: &mut State, tx: &Tx, block_timestamp: u64) -> Result<(), StfError> {
    validate_nonce(state, tx.from, tx.nonce)?;

    let result = match &tx.payload {
        TxPayload::Deposit(p) => apply_deposit(state, p),
        TxPayload::Withdraw(p) => apply_withdraw(state, tx.from, p),
        TxPayload::CreateDeal(p) => apply_create_deal(state, tx.from, p, block_timestamp),
        TxPayload::AcceptDeal(p) => apply_accept_deal(state, tx.from, p, block_timestamp),
        TxPayload::CancelDeal(p) => apply_cancel_deal(state, tx.from, p),
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

    let is_cross_chain = payload.chain_id_base != payload.chain_id_quote;

    let expires_at = payload.expires_at.map(|exp| {
        use zkclear_types::deal;
        let max_expiry = block_timestamp + deal::MAX_DEAL_DURATION_SECONDS;
        exp.min(max_expiry)
    });

    let deal = Deal {
        id: payload.deal_id,
        maker,
        taker: payload.taker,
        visibility: payload.visibility,
        asset_base: payload.asset_base,
        asset_quote: payload.asset_quote,
        chain_id_base: payload.chain_id_base,
        chain_id_quote: payload.chain_id_quote,
        amount_base: payload.amount_base,
        amount_remaining: payload.amount_base,
        price_quote_per_base: payload.price_quote_per_base,
        status: DealStatus::Pending,
        created_at: block_timestamp,
        expires_at,
        external_ref: payload.external_ref.clone(),
        is_cross_chain,
    };

    state.upsert_deal(deal);

    Ok(())
}

fn apply_accept_deal(
    state: &mut State,
    taker: Address,
    payload: &AcceptDeal,
    block_timestamp: u64,
) -> Result<(), StfError> {
    let (
        maker_addr,
        asset_base,
        asset_quote,
        chain_id_base,
        chain_id_quote,
        amount_remaining,
        price_quote_per_base,
        _expires_at,
        _visibility,
        _expected_taker,
    ) = {
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
            deal.asset_base,
            deal.asset_quote,
            deal.chain_id_base,
            deal.chain_id_quote,
            deal.amount_remaining,
            deal.price_quote_per_base,
            deal.expires_at,
            deal.visibility,
            deal.taker,
        )
    };

    let amount_to_fill = payload.amount.unwrap_or(amount_remaining);
    if amount_to_fill == 0 || amount_to_fill > amount_remaining {
        return Err(StfError::BalanceTooLow);
    }

    let amount_quote = amount_to_fill
        .checked_mul(price_quote_per_base)
        .ok_or(StfError::Overflow)?;

    ensure_balance(state, maker_addr, asset_base, amount_to_fill, chain_id_base)?;
    ensure_balance(state, taker, asset_quote, amount_quote, chain_id_quote)?;

    sub_balance(state, maker_addr, asset_base, amount_to_fill, chain_id_base)?;
    sub_balance(state, taker, asset_quote, amount_quote, chain_id_quote)?;

    add_balance(state, maker_addr, asset_quote, amount_quote, chain_id_quote);
    add_balance(state, taker, asset_base, amount_to_fill, chain_id_base);

    let deal = state
        .get_deal_mut(payload.deal_id)
        .ok_or(StfError::DealNotFound)?;
    deal.amount_remaining -= amount_to_fill;
    if deal.amount_remaining == 0 {
        deal.status = DealStatus::Settled;
    }

    Ok(())
}

fn apply_cancel_deal(
    state: &mut State,
    caller: Address,
    payload: &CancelDeal,
) -> Result<(), StfError> {
    let deal = state
        .get_deal_mut(payload.deal_id)
        .ok_or(StfError::DealNotFound)?;

    if deal.status != DealStatus::Pending {
        return Err(StfError::DealAlreadyClosed);
    }

    if deal.maker != caller {
        return Err(StfError::Unauthorized);
    }

    deal.status = DealStatus::Cancelled;

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

#[cfg(test)]
mod tests {
    use super::*;
    use zkclear_types::{Tx, TxKind, TxPayload};

    fn dummy_address(byte: u8) -> Address {
        [byte; 20]
    }

    fn default_chain_id() -> ChainId {
        zkclear_types::chain_ids::ETHEREUM
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
                asset_base: 0,
                asset_quote: 1,
                chain_id_base: default_chain_id(),
                chain_id_quote: default_chain_id(),
                amount_base: 1000,
                price_quote_per_base: 100,
                expires_at: None,
                external_ref: None,
            }),
        );
        apply_tx(&mut state, &create_deal_tx, block_timestamp).unwrap();

        let deal = state.get_deal(42).unwrap();
        assert_eq!(deal.maker, maker);
        assert_eq!(deal.amount_base, 1000);
        assert_eq!(deal.amount_remaining, 1000);
        assert_eq!(deal.status, DealStatus::Pending);
    }

    #[test]
    fn test_accept_deal() {
        let mut state = State::new();
        let maker = dummy_address(1);
        let taker = dummy_address(2);
        let block_timestamp = 1000;

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

        let create_deal = dummy_tx(
            maker,
            1,
            TxPayload::CreateDeal(CreateDeal {
                deal_id: 42,
                visibility: DealVisibility::Public,
                taker: None,
                asset_base: 0,
                asset_quote: 1,
                chain_id_base: default_chain_id(),
                chain_id_quote: default_chain_id(),
                amount_base: 1000,
                price_quote_per_base: 100,
                expires_at: None,
                external_ref: None,
            }),
        );
        apply_tx(&mut state, &create_deal, block_timestamp).unwrap();

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
        assert_eq!(deal.amount_remaining, 0);

        let maker_account = state.get_account_by_address(maker).unwrap();
        let taker_account = state.get_account_by_address(taker).unwrap();

        let maker_quote_balance = maker_account
            .balances
            .iter()
            .find(|b| b.asset_id == 1)
            .map(|b| b.amount)
            .unwrap_or(0);
        assert_eq!(maker_quote_balance, 100000);

        let taker_base_balance = taker_account
            .balances
            .iter()
            .find(|b| b.asset_id == 0)
            .map(|b| b.amount)
            .unwrap_or(0);
        assert_eq!(taker_base_balance, 1000);
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
}

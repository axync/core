use std::collections::HashMap;
use zkclear_types::{Account, AccountId, Address, Deal, DealId};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub accounts: HashMap<AccountId, Account>,
    pub deals: HashMap<DealId, Deal>,
    pub account_index: HashMap<Address, AccountId>,
    pub next_account_id: AccountId,
}

impl State {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            deals: HashMap::new(),
            account_index: HashMap::new(),
            next_account_id: 0,
        }
    }

    pub fn get_account(&self, id: AccountId) -> Option<&Account> {
        self.accounts.get(&id)
    }

    pub fn get_account_mut(&mut self, id: AccountId) -> Option<&mut Account> {
        self.accounts.get_mut(&id)
    }

    pub fn upsert_account(&mut self, account: Account) {
        self.account_index.insert(account.owner, account.id);
        self.accounts.insert(account.id, account);
    }

    pub fn get_deal(&self, id: DealId) -> Option<&Deal> {
        self.deals.get(&id)
    }

    pub fn get_deal_mut(&mut self, id: DealId) -> Option<&mut Deal> {
        self.deals.get_mut(&id)
    }

    pub fn upsert_deal(&mut self, deal: Deal) {
        self.deals.insert(deal.id, deal);
    }

    pub fn get_or_create_account_by_owner(&mut self, owner: Address) -> &mut Account {
        if let Some(id) = self.account_index.get(&owner).cloned() {
            return self.accounts.get_mut(&id).expect("inconsistent state");
        }

        let id = self.next_account_id;
        self.next_account_id = self.next_account_id.wrapping_add(1);

        let account = Account {
            id,
            owner,
            balances: Vec::new(),
            nonce: 0,
            created_at: 0,
        };

        self.accounts.insert(id, account);
        self.account_index.insert(owner, id);
        self.accounts.get_mut(&id).expect("just inserted")
    }

    pub fn get_account_by_address(&self, address: Address) -> Option<&Account> {
        self.account_index
            .get(&address)
            .and_then(|id| self.accounts.get(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zkclear_types::{Balance, Deal, DealStatus, DealVisibility};

    fn dummy_address(byte: u8) -> Address {
        [byte; 20]
    }

    #[test]
    fn test_new_state() {
        let state = State::new();
        assert_eq!(state.accounts.len(), 0);
        assert_eq!(state.deals.len(), 0);
        assert_eq!(state.next_account_id, 0);
    }

    #[test]
    fn test_get_or_create_account_by_owner() {
        let mut state = State::new();
        let addr = dummy_address(1);

        let account = state.get_or_create_account_by_owner(addr);
        assert_eq!(account.owner, addr);
        assert_eq!(account.id, 0);
        assert_eq!(account.balances.len(), 0);
        assert_eq!(account.nonce, 0);

        let account2 = state.get_or_create_account_by_owner(addr);
        assert_eq!(account2.id, 0);
        assert_eq!(state.accounts.len(), 1);
    }

    #[test]
    fn test_get_account_by_address() {
        let mut state = State::new();
        let addr = dummy_address(1);

        state.get_or_create_account_by_owner(addr);

        let account = state.get_account_by_address(addr);
        assert!(account.is_some());
        assert_eq!(account.unwrap().owner, addr);

        let unknown_addr = dummy_address(99);
        assert!(state.get_account_by_address(unknown_addr).is_none());
    }

    #[test]
    fn test_upsert_account() {
        let mut state = State::new();
        let addr = dummy_address(1);

        let account = Account {
            id: 0,
            owner: addr,
            balances: vec![Balance {
                asset_id: 0,
                amount: 100,
                chain_id: zkclear_types::chain_ids::ETHEREUM,
            }],
            nonce: 5,
            created_at: 1000,
        };

        state.upsert_account(account);

        let retrieved = state.get_account(0);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().nonce, 5);
        assert_eq!(retrieved.unwrap().balances.len(), 1);
    }

    #[test]
    fn test_upsert_deal() {
        let mut state = State::new();
        let maker = dummy_address(1);

        let deal = Deal {
            id: 42,
            maker,
            taker: None,
            asset_base: 0,
            asset_quote: 1,
            chain_id_base: zkclear_types::chain_ids::ETHEREUM,
            chain_id_quote: zkclear_types::chain_ids::ETHEREUM,
            amount_base: 1000,
            amount_remaining: 1000,
            price_quote_per_base: 100,
            status: DealStatus::Pending,
            visibility: DealVisibility::Public,
            created_at: 1000,
            expires_at: None,
            external_ref: None,
            is_cross_chain: false,
        };

        state.upsert_deal(deal);

        let retrieved = state.get_deal(42);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().maker, maker);
        assert_eq!(retrieved.unwrap().amount_base, 1000);
    }

    #[test]
    fn test_multiple_accounts() {
        let mut state = State::new();
        let addr1 = dummy_address(1);
        let addr2 = dummy_address(2);

        let acc1 = state.get_or_create_account_by_owner(addr1);
        assert_eq!(acc1.id, 0);

        let acc2 = state.get_or_create_account_by_owner(addr2);
        assert_eq!(acc2.id, 1);

        assert_eq!(state.accounts.len(), 2);
    }
}

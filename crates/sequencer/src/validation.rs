use k256::{
    ecdsa::{RecoveryId, Signature, VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey,
};
use sha3::{Digest, Keccak256};
use zkclear_state::State;
use zkclear_types::{Address, Tx, TxKind};

#[derive(Debug)]
pub enum ValidationError {
    InvalidSignature,
    InvalidNonce,
    SignatureRecoveryFailed,
}

pub fn validate_tx(state: &State, tx: &Tx) -> Result<(), ValidationError> {
    verify_signature(tx)?;
    check_nonce(state, tx)?;
    Ok(())
}

fn verify_signature(tx: &Tx) -> Result<(), ValidationError> {
    let recovered_address = recover_address(tx)?;

    if recovered_address != tx.from {
        return Err(ValidationError::InvalidSignature);
    }

    Ok(())
}

fn recover_address(tx: &Tx) -> Result<Address, ValidationError> {
    let message = tx_hash(tx);
    let message_hash = Keccak256::digest(&message);

    let sig_bytes = tx.signature;

    let mut r_bytes = [0u8; 32];
    r_bytes.copy_from_slice(&sig_bytes[0..32]);
    let r = k256::FieldBytes::from(r_bytes);

    let mut s_bytes = [0u8; 32];
    s_bytes.copy_from_slice(&sig_bytes[32..64]);
    let s = k256::FieldBytes::from(s_bytes);

    let v = sig_bytes[64];

    let recovery_id =
        RecoveryId::try_from(v % 27).map_err(|_| ValidationError::SignatureRecoveryFailed)?;

    let signature =
        Signature::from_scalars(r, s).map_err(|_| ValidationError::SignatureRecoveryFailed)?;

    let verifying_key = VerifyingKey::recover_from_prehash(&message_hash, &signature, recovery_id)
        .map_err(|_| ValidationError::SignatureRecoveryFailed)?;

    let public_key = PublicKey::from(&verifying_key);
    let encoded_point = public_key.to_encoded_point(false);
    let public_key_bytes = encoded_point.as_bytes();

    let hash = Keccak256::digest(&public_key_bytes[1..]);
    let mut address = [0u8; 20];
    address.copy_from_slice(&hash[12..]);

    Ok(address)
}

fn tx_hash(tx: &Tx) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&tx.from);
    data.extend_from_slice(&tx.nonce.to_le_bytes());

    let kind_byte = match tx.kind {
        TxKind::Deposit => 0u8,
        TxKind::Withdraw => 1u8,
        TxKind::CreateDeal => 2u8,
        TxKind::AcceptDeal => 3u8,
        TxKind::CancelDeal => 4u8,
    };
    data.push(kind_byte);

    match &tx.payload {
        zkclear_types::TxPayload::Deposit(p) => {
            data.extend_from_slice(&p.tx_hash);
            data.extend_from_slice(&p.account);
            data.extend_from_slice(&p.asset_id.to_le_bytes());
            data.extend_from_slice(&p.amount.to_le_bytes());
            data.extend_from_slice(&p.chain_id.to_le_bytes());
        }
        zkclear_types::TxPayload::Withdraw(p) => {
            data.extend_from_slice(&p.asset_id.to_le_bytes());
            data.extend_from_slice(&p.amount.to_le_bytes());
            data.extend_from_slice(&p.to);
            data.extend_from_slice(&p.chain_id.to_le_bytes());
        }
        zkclear_types::TxPayload::CreateDeal(p) => {
            data.extend_from_slice(&p.deal_id.to_le_bytes());
            data.push(p.visibility as u8);
            if let Some(taker) = p.taker {
                data.push(1);
                data.extend_from_slice(&taker);
            } else {
                data.push(0);
            }
            data.extend_from_slice(&p.asset_base.to_le_bytes());
            data.extend_from_slice(&p.asset_quote.to_le_bytes());
            data.extend_from_slice(&p.chain_id_base.to_le_bytes());
            data.extend_from_slice(&p.chain_id_quote.to_le_bytes());
            data.extend_from_slice(&p.amount_base.to_le_bytes());
            data.extend_from_slice(&p.price_quote_per_base.to_le_bytes());
        }
        zkclear_types::TxPayload::AcceptDeal(p) => {
            data.extend_from_slice(&p.deal_id.to_le_bytes());
            if let Some(amount) = p.amount {
                data.push(1);
                data.extend_from_slice(&amount.to_le_bytes());
            } else {
                data.push(0);
            }
        }
        zkclear_types::TxPayload::CancelDeal(p) => {
            data.extend_from_slice(&p.deal_id.to_le_bytes());
        }
    }

    let prefix = b"\x19Ethereum Signed Message:\n";
    let message_len = data.len();
    let mut prefixed = Vec::new();
    prefixed.extend_from_slice(prefix);
    prefixed.extend_from_slice(message_len.to_string().as_bytes());
    prefixed.extend_from_slice(&data);

    prefixed
}

fn check_nonce(state: &State, tx: &Tx) -> Result<(), ValidationError> {
    let account = state.get_account_by_address(tx.from);
    let expected_nonce = account.map(|a| a.nonce).unwrap_or(0);

    if tx.nonce != expected_nonce {
        return Err(ValidationError::InvalidNonce);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zkclear_types::{Deposit, Tx, TxKind, TxPayload};

    fn dummy_address(byte: u8) -> Address {
        [byte; 20]
    }

    fn dummy_tx_with_nonce(from: Address, nonce: u64) -> Tx {
        Tx {
            id: 0,
            from,
            nonce,
            kind: TxKind::Deposit,
            payload: TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: from,
                asset_id: 0,
                amount: 100,
                chain_id: 1,
            }),
            signature: [0u8; 65],
        }
    }

    #[test]
    fn test_validate_nonce_new_account() {
        let state = State::new();
        let addr = dummy_address(1);
        let tx = dummy_tx_with_nonce(addr, 0);

        let result = check_nonce(&state, &tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_nonce_correct() {
        let mut state = State::new();
        let addr = dummy_address(1);

        let account = state.get_or_create_account_by_owner(addr);
        account.nonce = 5;

        let tx = dummy_tx_with_nonce(addr, 5);
        let result = check_nonce(&state, &tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_nonce_incorrect() {
        let mut state = State::new();
        let addr = dummy_address(1);

        let account = state.get_or_create_account_by_owner(addr);
        account.nonce = 5;

        let tx = dummy_tx_with_nonce(addr, 3);
        let result = check_nonce(&state, &tx);
        assert!(matches!(result, Err(ValidationError::InvalidNonce)));
    }

    #[test]
    fn test_validate_nonce_too_high() {
        let mut state = State::new();
        let addr = dummy_address(1);

        let account = state.get_or_create_account_by_owner(addr);
        account.nonce = 5;

        let tx = dummy_tx_with_nonce(addr, 10);
        let result = check_nonce(&state, &tx);
        assert!(matches!(result, Err(ValidationError::InvalidNonce)));
    }

    #[test]
    fn test_check_nonce_sequential() {
        let mut state = State::new();
        let addr = dummy_address(1);

        {
            let account = state.get_or_create_account_by_owner(addr);
            account.nonce = 0;
        }

        let tx1 = dummy_tx_with_nonce(addr, 0);
        assert!(check_nonce(&state, &tx1).is_ok());

        {
            let account = state.get_or_create_account_by_owner(addr);
            account.nonce = 1;
        }

        let tx2 = dummy_tx_with_nonce(addr, 1);
        assert!(check_nonce(&state, &tx2).is_ok());
    }
}

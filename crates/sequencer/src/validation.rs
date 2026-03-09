use k256::{
    ecdsa::{RecoveryId, Signature, VerifyingKey},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey,
};
use sha3::{Digest, Keccak256};
use axync_state::State;
use axync_types::{Address, Tx};

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
    let message = eip712_signing_input(tx);
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

// ── EIP-712 Typed Data Signing ──────────────────────────────────────────────
//
// Domain: EIP712Domain(string name, string version)
//   name = "Axync", version = "1"
//   No chainId — this is a cross-chain sequencer.
//
// Struct types (each includes from + nonce):
//   Deposit(address from,uint64 nonce,bytes32 txHash,address account,uint16 assetId,uint128 amount,uint64 chainId)
//   Withdraw(address from,uint64 nonce,uint16 assetId,uint128 amount,address to,uint64 chainId)
//   CreateDeal(address from,uint64 nonce,uint64 dealId,string visibility,address taker,uint16 assetBase,uint16 assetQuote,uint64 chainIdBase,uint64 chainIdQuote,uint128 amountBase,uint128 priceQuotePerBase)
//   AcceptDeal(address from,uint64 nonce,uint64 dealId)
//   CancelDeal(address from,uint64 nonce,uint64 dealId)

/// Build the EIP-712 signing input: \x19\x01 ‖ domainSeparator ‖ structHash
fn eip712_signing_input(tx: &Tx) -> Vec<u8> {
    let domain_separator = compute_domain_separator();
    let struct_hash = compute_struct_hash(tx);

    let mut result = Vec::with_capacity(66);
    result.push(0x19);
    result.push(0x01);
    result.extend_from_slice(&domain_separator);
    result.extend_from_slice(&struct_hash);
    result
}

fn compute_domain_separator() -> [u8; 32] {
    let type_hash = Keccak256::digest(b"EIP712Domain(string name,string version)");
    let name_hash = Keccak256::digest(b"Axync");
    let version_hash = Keccak256::digest(b"1");

    let mut data = Vec::with_capacity(96);
    data.extend_from_slice(&type_hash);
    data.extend_from_slice(&name_hash);
    data.extend_from_slice(&version_hash);

    let result = Keccak256::digest(&data);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

fn compute_struct_hash(tx: &Tx) -> [u8; 32] {
    let (type_hash, encoded_fields) = match &tx.payload {
        axync_types::TxPayload::Deposit(p) => {
            let type_hash = Keccak256::digest(
                b"Deposit(address from,uint64 nonce,bytes32 txHash,address account,uint16 assetId,uint128 amount,uint64 chainId)"
            );
            let mut fields = Vec::new();
            fields.extend_from_slice(&encode_address(&tx.from));
            fields.extend_from_slice(&encode_uint64(tx.nonce));
            fields.extend_from_slice(&encode_bytes32(&p.tx_hash));
            fields.extend_from_slice(&encode_address(&p.account));
            fields.extend_from_slice(&encode_uint16(p.asset_id));
            fields.extend_from_slice(&encode_uint128(p.amount));
            fields.extend_from_slice(&encode_uint64(p.chain_id));
            (type_hash, fields)
        }
        axync_types::TxPayload::Withdraw(p) => {
            let type_hash = Keccak256::digest(
                b"Withdraw(address from,uint64 nonce,uint16 assetId,uint128 amount,address to,uint64 chainId)"
            );
            let mut fields = Vec::new();
            fields.extend_from_slice(&encode_address(&tx.from));
            fields.extend_from_slice(&encode_uint64(tx.nonce));
            fields.extend_from_slice(&encode_uint16(p.asset_id));
            fields.extend_from_slice(&encode_uint128(p.amount));
            fields.extend_from_slice(&encode_address(&p.to));
            fields.extend_from_slice(&encode_uint64(p.chain_id));
            (type_hash, fields)
        }
        axync_types::TxPayload::CreateDeal(p) => {
            let type_hash = Keccak256::digest(
                b"CreateDeal(address from,uint64 nonce,uint64 dealId,string visibility,address taker,uint16 assetBase,uint16 assetQuote,uint64 chainIdBase,uint64 chainIdQuote,uint128 amountBase,uint128 priceQuotePerBase)"
            );
            let mut fields = Vec::new();
            fields.extend_from_slice(&encode_address(&tx.from));
            fields.extend_from_slice(&encode_uint64(tx.nonce));
            fields.extend_from_slice(&encode_uint64(p.deal_id));
            let vis_str = match p.visibility {
                axync_types::DealVisibility::Public => "Public",
                axync_types::DealVisibility::Direct => "Direct",
            };
            fields.extend_from_slice(&encode_string(vis_str));
            let taker_addr = p.taker.unwrap_or([0u8; 20]);
            fields.extend_from_slice(&encode_address(&taker_addr));
            fields.extend_from_slice(&encode_uint16(p.asset_base));
            fields.extend_from_slice(&encode_uint16(p.asset_quote));
            fields.extend_from_slice(&encode_uint64(p.chain_id_base));
            fields.extend_from_slice(&encode_uint64(p.chain_id_quote));
            fields.extend_from_slice(&encode_uint128(p.amount_base));
            fields.extend_from_slice(&encode_uint128(p.price_quote_per_base));
            (type_hash, fields)
        }
        axync_types::TxPayload::AcceptDeal(p) => {
            let type_hash = Keccak256::digest(
                b"AcceptDeal(address from,uint64 nonce,uint64 dealId)"
            );
            let mut fields = Vec::new();
            fields.extend_from_slice(&encode_address(&tx.from));
            fields.extend_from_slice(&encode_uint64(tx.nonce));
            fields.extend_from_slice(&encode_uint64(p.deal_id));
            (type_hash, fields)
        }
        axync_types::TxPayload::CancelDeal(p) => {
            let type_hash = Keccak256::digest(
                b"CancelDeal(address from,uint64 nonce,uint64 dealId)"
            );
            let mut fields = Vec::new();
            fields.extend_from_slice(&encode_address(&tx.from));
            fields.extend_from_slice(&encode_uint64(tx.nonce));
            fields.extend_from_slice(&encode_uint64(p.deal_id));
            (type_hash, fields)
        }
    };

    let mut data = Vec::new();
    data.extend_from_slice(&type_hash);
    data.extend_from_slice(&encoded_fields);

    let result = Keccak256::digest(&data);
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

// ── ABI Encoding Helpers (EIP-712: big-endian, 32-byte words) ──────────────

fn encode_address(addr: &[u8; 20]) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[12..32].copy_from_slice(addr);
    result
}

fn encode_uint64(val: u64) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[24..32].copy_from_slice(&val.to_be_bytes());
    result
}

fn encode_uint16(val: u16) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[30..32].copy_from_slice(&val.to_be_bytes());
    result
}

fn encode_uint128(val: u128) -> [u8; 32] {
    let mut result = [0u8; 32];
    result[16..32].copy_from_slice(&val.to_be_bytes());
    result
}

fn encode_bytes32(val: &[u8; 32]) -> [u8; 32] {
    *val
}

fn encode_string(val: &str) -> [u8; 32] {
    let hash = Keccak256::digest(val.as_bytes());
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
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
    use axync_types::{Deposit, Tx, TxKind, TxPayload};

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

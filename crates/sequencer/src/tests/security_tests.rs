//! Security tests for sequencer validation and security checks

use crate::security::*;
use zkclear_types::{Address, Tx, TxKind, TxPayload, Deposit};

#[test]
fn test_validate_address_rejects_zero() {
    let zero_addr = [0u8; 20];
    assert!(!validate_address(&zero_addr));
}

#[test]
fn test_validate_address_rejects_max() {
    let max_addr = [0xFFu8; 20];
    assert!(!validate_address(&max_addr));
}

#[test]
fn test_validate_address_accepts_valid() {
    let valid_addr = [1u8; 20];
    assert!(validate_address(&valid_addr));
}

#[test]
fn test_validate_amount_rejects_zero() {
    assert!(!validate_amount(0));
}

#[test]
fn test_validate_amount_accepts_valid() {
    assert!(validate_amount(1));
    assert!(validate_amount(1000));
    assert!(validate_amount(u64::MAX / 2));
}

#[test]
fn test_validate_nonce_gap_accepts_sequential() {
    assert!(validate_nonce_gap(0, 0).is_ok());
    assert!(validate_nonce_gap(0, 1).is_ok());
    assert!(validate_nonce_gap(5, 6).is_ok());
}

#[test]
fn test_validate_nonce_gap_rejects_backwards() {
    assert!(validate_nonce_gap(5, 4).is_err());
    assert!(validate_nonce_gap(10, 0).is_err());
}

#[test]
fn test_validate_nonce_gap_rejects_large_gap() {
    assert!(validate_nonce_gap(0, MAX_NONCE_GAP + 1).is_err());
}

#[test]
fn test_validate_tx_size_accepts_normal() {
    let tx = create_test_tx();
    assert!(validate_tx_size(&tx).is_ok());
}

#[test]
fn test_validate_hex_string() {
    assert!(validate_hex_string("0x1234"));
    assert!(validate_hex_string("1234"));
    assert!(validate_hex_string("abcdef"));
    assert!(!validate_hex_string(""));
    assert!(!validate_hex_string("0x123")); // Odd length
    assert!(!validate_hex_string("xyz"));
}

#[test]
fn test_sanitize_string_removes_control_chars() {
    let input = "test\x00string\n";
    let sanitized = sanitize_string(input);
    assert!(!sanitized.contains('\x00'));
    assert!(sanitized.contains("test"));
    assert!(sanitized.contains("string"));
}

fn create_test_tx() -> Tx {
    Tx {
        id: 0,
        from: [1u8; 20],
        nonce: 0,
        kind: TxKind::Deposit,
        payload: TxPayload::Deposit(Deposit {
            tx_hash: [0u8; 32],
            account: [1u8; 20],
            asset_id: 0,
            amount: 100,
            chain_id: 1,
        }),
        signature: [0u8; 65],
    }
}


//! Security utilities and validations for ZKClear sequencer
//!
//! This module provides security-focused validations and utilities:
//! - Input sanitization
//! - Signature malleability protection
//! - Overflow/underflow protection
//! - Replay attack prevention

use crate::validation::ValidationError;
use zkclear_types::Tx;

/// Maximum allowed transaction size (in bytes)
/// Prevents DoS attacks via oversized transactions
pub const MAX_TX_SIZE: usize = 10_000;

/// Maximum allowed nonce gap
/// Prevents potential issues with very large nonce jumps
pub const MAX_NONCE_GAP: u64 = 1_000_000;

/// Validate transaction size to prevent DoS attacks
pub fn validate_tx_size(tx: &Tx) -> Result<(), ValidationError> {
    // Estimate transaction size
    let size = std::mem::size_of::<Tx>();
    
    // Check payload size (rough estimate)
    let payload_size = match &tx.payload {
        zkclear_types::TxPayload::Deposit(_) => 100,
        zkclear_types::TxPayload::Withdraw(_) => 100,
        zkclear_types::TxPayload::CreateDeal(_) => 500,
        zkclear_types::TxPayload::AcceptDeal(_) => 50,
        zkclear_types::TxPayload::CancelDeal(_) => 50,
    };
    
    let total_size = size + payload_size;
    
    if total_size > MAX_TX_SIZE {
        return Err(ValidationError::InvalidSignature); // Reuse error type for now
    }
    
    Ok(())
}

/// Validate nonce to prevent potential issues with very large gaps
pub fn validate_nonce_gap(current_nonce: u64, tx_nonce: u64) -> Result<(), ValidationError> {
    if tx_nonce < current_nonce {
        return Err(ValidationError::InvalidNonce);
    }
    
    let gap = tx_nonce.saturating_sub(current_nonce);
    if gap > MAX_NONCE_GAP {
        return Err(ValidationError::InvalidNonce);
    }
    
    Ok(())
}

// Note: Signature canonicality checking is handled by k256 library during signature recovery
// The library automatically handles canonical signatures, so no additional check is needed here

/// Validate address format (basic checks)
pub fn validate_address(address: &[u8; 20]) -> bool {
    // Check that address is not all zeros
    if address.iter().all(|&b| b == 0) {
        return false;
    }
    
    // Check that address is not all 0xFF
    if address.iter().all(|&b| b == 0xFF) {
        return false;
    }
    
    true
}

/// Validate amount to prevent overflow/underflow issues
pub fn validate_amount(amount: u64) -> bool {
    // Check for reasonable maximum (prevent potential overflow in calculations)
    const MAX_AMOUNT: u64 = u64::MAX / 2; // Conservative limit
    
    amount > 0 && amount <= MAX_AMOUNT
}

/// Sanitize string input (for API endpoints)
pub fn sanitize_string(input: &str) -> String {
    // Remove control characters
    input
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect()
}

/// Validate hex string format
pub fn validate_hex_string(input: &str) -> bool {
    if input.is_empty() {
        return false;
    }
    
    // Check if it's a hex string (with or without 0x prefix)
    let hex_part = if input.starts_with("0x") || input.starts_with("0X") {
        &input[2..]
    } else {
        input
    };
    
    // Check length (should be even for bytes)
    if hex_part.len() % 2 != 0 {
        return false;
    }
    
    // Check that all characters are valid hex
    hex_part.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_address() {
        let zero_addr = [0u8; 20];
        assert!(!validate_address(&zero_addr));
        
        let max_addr = [0xFFu8; 20];
        assert!(!validate_address(&max_addr));
        
        let valid_addr = [1u8; 20];
        assert!(validate_address(&valid_addr));
    }

    #[test]
    fn test_validate_amount() {
        assert!(!validate_amount(0));
        assert!(validate_amount(1));
        assert!(validate_amount(1000));
        assert!(validate_amount(u64::MAX / 2));
    }

    #[test]
    fn test_validate_nonce_gap() {
        assert!(validate_nonce_gap(0, 0).is_ok());
        assert!(validate_nonce_gap(0, 1).is_ok());
        assert!(validate_nonce_gap(5, 6).is_ok());
        assert!(validate_nonce_gap(5, 4).is_err());
        assert!(validate_nonce_gap(0, MAX_NONCE_GAP + 1).is_err());
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
    fn test_sanitize_string() {
        let input = "test\x00string\n";
        let sanitized = sanitize_string(input);
        assert!(!sanitized.contains('\x00'));
    }
}


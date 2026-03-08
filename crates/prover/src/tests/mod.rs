//! Comprehensive test suite for ZK proof generation and verification
//!
//! This module contains tests for:
//! - STARK proof generation
//! - SNARK wrapping
//! - Proof verification
//! - End-to-end flow
//! - Validation of generated proofs
//! - Testing with different block sizes

#[cfg(feature = "stark")]
mod stark_tests;

#[cfg(feature = "arkworks")]
mod snark_tests;

#[cfg(any(feature = "stark", feature = "arkworks"))]
mod integration_tests;

#[cfg(any(feature = "stark", feature = "arkworks"))]
mod validation_tests;

#[cfg(any(feature = "stark", feature = "arkworks"))]
mod performance_tests;

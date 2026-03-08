use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProverError {
    #[error("Merkle tree error: {0}")]
    MerkleTree(String),

    #[error("STARK proof generation failed: {0}")]
    StarkProof(String),

    #[error("SNARK proof generation failed: {0}")]
    SnarkProof(String),

    #[error("Invalid state root: {0}")]
    InvalidStateRoot(String),

    #[error("Invalid withdrawals root: {0}")]
    InvalidWithdrawalsRoot(String),

    #[error("Nullifier generation failed: {0}")]
    NullifierGeneration(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

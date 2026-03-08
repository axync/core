//! Key management for Groth16 proving and verifying keys
//!
//! This module handles generation, serialization, and persistence of
//! Groth16 proving and verifying keys to avoid expensive regeneration.

#[cfg(feature = "arkworks")]
use crate::circuit::StarkProofVerifierCircuit;
#[cfg(feature = "arkworks")]
use crate::error::ProverError;
#[cfg(feature = "arkworks")]
use ark_bn254::Bn254;
#[cfg(feature = "arkworks")]
use ark_groth16::{Groth16, ProvingKey, VerifyingKey};
#[cfg(feature = "arkworks")]
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};
#[cfg(feature = "arkworks")]
use ark_std::rand::rngs::StdRng;
#[cfg(feature = "arkworks")]
use ark_std::rand::SeedableRng;
#[cfg(feature = "arkworks")]
use std::fs;
#[cfg(feature = "arkworks")]
use std::path::{Path, PathBuf};

/// Default directory for storing Groth16 keys
#[cfg(feature = "arkworks")]
const DEFAULT_KEYS_DIR: &str = "./keys";

/// Filenames for key storage
#[cfg(feature = "arkworks")]
const PROVING_KEY_FILE: &str = "groth16_proving_key.bin";
#[cfg(feature = "arkworks")]
const VERIFYING_KEY_FILE: &str = "groth16_verifying_key.bin";

/// Key manager for Groth16 keys
#[cfg(feature = "arkworks")]
pub struct KeyManager {
    keys_dir: PathBuf,
    proving_key: Option<ProvingKey<Bn254>>,
    verifying_key: Option<VerifyingKey<Bn254>>,
}

#[cfg(feature = "arkworks")]
impl KeyManager {
    /// Create a new key manager with the specified keys directory
    pub fn new(keys_dir: Option<PathBuf>) -> Self {
        let keys_dir = keys_dir.unwrap_or_else(|| PathBuf::from(DEFAULT_KEYS_DIR));
        Self {
            keys_dir,
            proving_key: None,
            verifying_key: None,
        }
    }

    /// Load keys from disk, or generate new ones if they don't exist
    pub fn load_or_generate(&mut self, force_regenerate: bool) -> Result<(), ProverError> {
        let proving_key_path = self.keys_dir.join(PROVING_KEY_FILE);
        let verifying_key_path = self.keys_dir.join(VERIFYING_KEY_FILE);

        // Check if keys exist
        let keys_exist = proving_key_path.exists() && verifying_key_path.exists();

        if keys_exist && !force_regenerate {
            // Load existing keys
            eprintln!("   Loading existing Groth16 keys...");
            self.load_keys()?;
            eprintln!("   Keys loaded");
            return Ok(());
        }

        // Generate new keys
        eprintln!("   Generating new Groth16 keys (this may take 30-60 seconds)...");
        eprintln!("   Please wait, this is a one-time operation...");
        self.generate_keys()?;

        // Save keys to disk
        eprintln!("   Saving keys to disk...");
        self.save_keys()?;
        eprintln!("   Keys generated and saved");

        Ok(())
    }

    /// Generate new proving and verifying keys
    fn generate_keys(&mut self) -> Result<(), ProverError> {
        // Create a dummy circuit to generate keys
        // The circuit structure is fixed, so we can use dummy values
        // IMPORTANT: The circuit structure must match exactly when generating proofs
        // - public_inputs: always 96 bytes (3 * 32 bytes for roots)
        // - stark_proof: always at least 200 bytes (will be padded if smaller)
        let dummy_circuit = StarkProofVerifierCircuit {
            public_inputs: vec![0u8; 96], // 3 * 32 bytes for roots
            stark_proof: vec![0u8; 200],  // Dummy proof (minimum size for minimal STARK proof)
        };

        // Use deterministic seed for key generation
        // In production, this should use secure randomness
        let mut seed = [0u8; 32];
        // Use a fixed seed for reproducibility (can be changed for production)
        // "ZKClearPK" is 9 bytes, but we need 8, so use first 8 bytes
        seed[0..8].copy_from_slice(&b"ZKClearPK"[0..8]); // Fixed seed for key generation

        let mut rng = StdRng::from_seed(seed);

        // Generate proving key
        let pk = Groth16::<Bn254>::generate_random_parameters_with_reduction(
            dummy_circuit.clone(),
            &mut rng,
        )
        .map_err(|e| ProverError::SnarkProof(format!("Failed to generate proving key: {:?}", e)))?;

        let vk = pk.vk.clone();

        self.proving_key = Some(pk);
        self.verifying_key = Some(vk);

        Ok(())
    }

    /// Save keys to disk
    fn save_keys(&self) -> Result<(), ProverError> {
        // Create keys directory if it doesn't exist
        fs::create_dir_all(&self.keys_dir).map_err(|e| {
            ProverError::Serialization(format!("Failed to create keys directory: {}", e))
        })?;

        let proving_key_path = self.keys_dir.join(PROVING_KEY_FILE);
        let verifying_key_path = self.keys_dir.join(VERIFYING_KEY_FILE);

        // Save proving key
        if let Some(ref pk) = self.proving_key {
            let mut pk_bytes = Vec::new();
            pk.serialize_with_mode(&mut pk_bytes, Compress::Yes)
                .map_err(|e| {
                    ProverError::Serialization(format!("Failed to serialize proving key: {}", e))
                })?;

            fs::write(&proving_key_path, &pk_bytes).map_err(|e| {
                ProverError::Serialization(format!("Failed to write proving key: {}", e))
            })?;
        }

        // Save verifying key
        if let Some(ref vk) = self.verifying_key {
            let mut vk_bytes = Vec::new();
            vk.serialize_with_mode(&mut vk_bytes, Compress::Yes)
                .map_err(|e| {
                    ProverError::Serialization(format!("Failed to serialize verifying key: {}", e))
                })?;

            fs::write(&verifying_key_path, &vk_bytes).map_err(|e| {
                ProverError::Serialization(format!("Failed to write verifying key: {}", e))
            })?;
        }

        Ok(())
    }

    /// Load keys from disk
    fn load_keys(&mut self) -> Result<(), ProverError> {
        let proving_key_path = self.keys_dir.join(PROVING_KEY_FILE);
        let verifying_key_path = self.keys_dir.join(VERIFYING_KEY_FILE);

        // Load proving key
        let pk_bytes = fs::read(&proving_key_path).map_err(|e| {
            ProverError::Serialization(format!("Failed to read proving key: {}", e))
        })?;

        let pk =
            ProvingKey::<Bn254>::deserialize_with_mode(&pk_bytes[..], Compress::Yes, Validate::Yes)
                .map_err(|e| {
                    ProverError::Serialization(format!("Failed to deserialize proving key: {}", e))
                })?;

        // Load verifying key
        let vk_bytes = fs::read(&verifying_key_path).map_err(|e| {
            ProverError::Serialization(format!("Failed to read verifying key: {}", e))
        })?;

        let vk = VerifyingKey::<Bn254>::deserialize_with_mode(
            &vk_bytes[..],
            Compress::Yes,
            Validate::Yes,
        )
        .map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize verifying key: {}", e))
        })?;

        self.proving_key = Some(pk);
        self.verifying_key = Some(vk);

        Ok(())
    }

    /// Get the proving key (must be loaded first)
    pub fn proving_key(&self) -> Result<&ProvingKey<Bn254>, ProverError> {
        self.proving_key
            .as_ref()
            .ok_or_else(|| ProverError::SnarkProof("Proving key not loaded".to_string()))
    }

    /// Get the verifying key (must be loaded first)
    pub fn verifying_key(&self) -> Result<&VerifyingKey<Bn254>, ProverError> {
        self.verifying_key
            .as_ref()
            .ok_or_else(|| ProverError::SnarkProof("Verifying key not loaded".to_string()))
    }

    /// Get the keys directory path
    pub fn keys_dir(&self) -> &Path {
        &self.keys_dir
    }
}

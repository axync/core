//! Generate a Groth16 proof and format it for Solidity contract submission
//!
//! This tool:
//! 1. Creates a test block with transactions
//! 2. Generates STARK proof using minimal STARK prover
//! 3. Wraps STARK proof in Groth16 SNARK
//! 4. Formats proof for Solidity contract submission

use std::fs;
use std::time::SystemTime;
use zkclear_prover::{Prover, ProverConfig};
use zkclear_state::State;
use zkclear_stf::apply_tx;
use zkclear_types::{Address, Block, Deposit, Tx, TxKind, TxPayload};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create runtime: {}", e))?;
    rt.block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let output_file = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("proof_for_solidity.js");

    println!("Initializing Prover...");

    // Initialize prover with keys
    let config = ProverConfig {
        groth16_keys_dir: Some("./keys".into()),
        force_regenerate_keys: false,
        use_placeholders: false,
    };

    let prover = Prover::new(config).map_err(|e| format!("Failed to create prover: {}", e))?;

    println!("Creating test block...");

    // Create a test block with some transactions
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let block = Block {
        id: 1,
        timestamp,
        transactions: vec![
            Tx {
                id: 0,
                from: Address::from([0x01; 20]),
                nonce: 0,
                kind: TxKind::Deposit,
                payload: TxPayload::Deposit(Deposit {
                    tx_hash: [0x01; 32],
                    account: Address::from([0x02; 20]),
                    asset_id: 1,
                    amount: 1000,
                    chain_id: 1,
                }),
                signature: [0u8; 65],
            },
            Tx {
                id: 1,
                from: Address::from([0x03; 20]),
                nonce: 0,
                kind: TxKind::Deposit,
                payload: TxPayload::Deposit(Deposit {
                    tx_hash: [0x03; 32],
                    account: Address::from([0x04; 20]),
                    asset_id: 1,
                    amount: 2000,
                    chain_id: 1,
                }),
                signature: [0u8; 65],
            },
        ],
        state_root: [0u8; 32],
        withdrawals_root: [0u8; 32],
        block_proof: vec![],
    };

    println!("Computing state roots...");

    // Create initial state
    let prev_state = State::new();

    // Apply transactions to get new state
    let mut new_state = prev_state.clone();
    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp)
            .map_err(|e| format!("Failed to apply tx: {:?}", e))?;
    }

    println!("Generating ZK proof...");

    // Generate proof
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .map_err(|e| format!("Failed to generate proof: {}", e))?;

    println!("Proof generated!");
    println!(
        "   - Prev state root: 0x{}",
        hex::encode(block_proof.prev_state_root)
    );
    println!(
        "   - New state root:  0x{}",
        hex::encode(block_proof.new_state_root)
    );
    println!(
        "   - Withdrawals root: 0x{}",
        hex::encode(block_proof.withdrawals_root)
    );
    println!("   - Proof size: {} bytes", block_proof.zk_proof.len());

    // Save proof to file for formatting
    let proof_file = "generated_proof.bin";
    fs::write(&proof_file, bincode::serialize(&block_proof.zk_proof)?)?;
    println!("Proof saved to: {}", proof_file);

    // Format proof for Solidity
    println!("Formatting proof for Solidity...");

    // Use the format_proof_for_solidity binary logic
    use ark_bn254::Bn254;
    use ark_groth16::Proof;
    use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Compress, Validate};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct SnarkProofWrapper {
        proof: Vec<u8>,
        public_inputs: Vec<u8>,
        version: u8,
    }

    let wrapper: SnarkProofWrapper = bincode::deserialize(&block_proof.zk_proof)?;

    if wrapper.version != 3 {
        return Err(format!("Unsupported proof version: {}", wrapper.version).into());
    }

    let groth16_proof =
        Proof::<Bn254>::deserialize_with_mode(&wrapper.proof[..], Compress::Yes, Validate::Yes)?;

    // Format proof as 256 bytes for Solidity
    let mut solidity_proof = Vec::new();

    // A point (G1): 64 bytes
    let mut a_x_bytes = Vec::new();
    let mut a_y_bytes = Vec::new();
    groth16_proof
        .a
        .x
        .serialize_with_mode(&mut a_x_bytes, Compress::No)
        .unwrap();
    groth16_proof
        .a
        .y
        .serialize_with_mode(&mut a_y_bytes, Compress::No)
        .unwrap();
    solidity_proof.extend_from_slice(&a_x_bytes[0..32]); // Take first 32 bytes
    solidity_proof.extend_from_slice(&a_y_bytes[0..32]);

    // B point (G2): 128 bytes
    let mut b_x_c0_bytes = Vec::new();
    let mut b_x_c1_bytes = Vec::new();
    let mut b_y_c0_bytes = Vec::new();
    let mut b_y_c1_bytes = Vec::new();
    groth16_proof
        .b
        .x
        .c0
        .serialize_with_mode(&mut b_x_c0_bytes, Compress::No)
        .unwrap();
    groth16_proof
        .b
        .x
        .c1
        .serialize_with_mode(&mut b_x_c1_bytes, Compress::No)
        .unwrap();
    groth16_proof
        .b
        .y
        .c0
        .serialize_with_mode(&mut b_y_c0_bytes, Compress::No)
        .unwrap();
    groth16_proof
        .b
        .y
        .c1
        .serialize_with_mode(&mut b_y_c1_bytes, Compress::No)
        .unwrap();
    solidity_proof.extend_from_slice(&b_x_c0_bytes[0..32]);
    solidity_proof.extend_from_slice(&b_x_c1_bytes[0..32]);
    solidity_proof.extend_from_slice(&b_y_c0_bytes[0..32]);
    solidity_proof.extend_from_slice(&b_y_c1_bytes[0..32]);

    // C point (G1): 64 bytes
    let mut c_x_bytes = Vec::new();
    let mut c_y_bytes = Vec::new();
    groth16_proof
        .c
        .x
        .serialize_with_mode(&mut c_x_bytes, Compress::No)
        .unwrap();
    groth16_proof
        .c
        .y
        .serialize_with_mode(&mut c_y_bytes, Compress::No)
        .unwrap();
    solidity_proof.extend_from_slice(&c_x_bytes[0..32]);
    solidity_proof.extend_from_slice(&c_y_bytes[0..32]);

    // Convert public inputs to 24 field elements
    let mut public_inputs_elements = Vec::new();
    for root_idx in 0..3 {
        let root_start = root_idx * 32;
        for i in 0..8 {
            let byte_start = root_start + (i * 4);
            let chunk = &wrapper.public_inputs[byte_start..byte_start + 4];
            let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            public_inputs_elements.push(value);
        }
    }

    // Generate JavaScript/TypeScript code for Hardhat test
    let mut output = String::new();
    output.push_str("// Generated proof for VerifierContract.submitBlockProof\n");
    output.push_str("// Generated at: ");
    output.push_str(
        &SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string(),
    );
    output.push_str("\n\n");

    output.push_str("const proof = \"0x");
    output.push_str(&hex::encode(&solidity_proof));
    output.push_str("\";\n\n");

    output.push_str("const publicInputs = [\n");
    for (i, elem) in public_inputs_elements.iter().enumerate() {
        output.push_str(&format!("  \"{}\"", elem));
        if i < public_inputs_elements.len() - 1 {
            output.push_str(",");
        }
        output.push_str("\n");
    }
    output.push_str("];\n\n");

    output.push_str("const prevStateRoot = \"0x");
    output.push_str(&hex::encode(block_proof.prev_state_root));
    output.push_str("\";\n\n");

    output.push_str("const newStateRoot = \"0x");
    output.push_str(&hex::encode(block_proof.new_state_root));
    output.push_str("\";\n\n");

    output.push_str("const withdrawalsRoot = \"0x");
    output.push_str(&hex::encode(block_proof.withdrawals_root));
    output.push_str("\";\n\n");

    output.push_str("const blockId = ");
    output.push_str(&block.id.to_string());
    output.push_str(";\n\n");

    output.push_str("// Usage in Hardhat test:\n");
    output.push_str("// await verifierContract.connect(sequencer).submitBlockProof(\n");
    output.push_str("//   blockId,\n");
    output.push_str("//   prevStateRoot,\n");
    output.push_str("//   newStateRoot,\n");
    output.push_str("//   withdrawalsRoot,\n");
    output.push_str("//   proof\n");
    output.push_str("// );\n");

    fs::write(output_file, output)?;
    println!("Formatted proof saved to: {}", output_file);
    println!("You can now use this in your Hardhat test!");

    Ok(())
}

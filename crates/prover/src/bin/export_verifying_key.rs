//! Utility to export Groth16 verifying key for Solidity deployment
//!
//! This binary extracts the verifying key from the saved key file and
//! outputs it in a format suitable for Solidity contract deployment.

#[cfg(feature = "arkworks")]
use ark_ff::BigInteger;

#[cfg(feature = "arkworks")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use zkclear_prover::keys::KeyManager;

    // Load verifying key
    let mut key_manager = KeyManager::new(None);
    key_manager.load_or_generate(false)?;
    let vk = key_manager.verifying_key()?;

    // Access verifying key fields
    // VerifyingKey in arkworks has these fields:
    // - alpha_g1: G1Affine
    // - beta_g2: G2Affine
    // - gamma_g2: G2Affine
    // - delta_g2: G2Affine
    // - gamma_abc_g1: Vec<G1Affine>

    // Extract verifying key components
    let alpha = &vk.alpha_g1;
    let beta = &vk.beta_g2;
    let gamma = &vk.gamma_g2;
    let delta = &vk.delta_g2;
    let gamma_abc = &vk.gamma_abc_g1;

    println!("// Groth16 Verifying Key for Solidity");
    println!("// Generated from Arkworks Groth16 keys");
    println!();

    let (alpha_x, alpha_y) = format_g1_point(alpha);
    println!("// Alpha (G1):");
    println!("alpha_X: {}", alpha_x);
    println!("alpha_Y: {}", alpha_y);
    println!();

    let (beta_x_c0, beta_x_c1, beta_y_c0, beta_y_c1) = format_g2_point(beta);
    println!("// Beta (G2):");
    println!("beta_X: [{}, {}]", beta_x_c0, beta_x_c1);
    println!("beta_Y: [{}, {}]", beta_y_c0, beta_y_c1);
    println!();

    let (gamma_x_c0, gamma_x_c1, gamma_y_c0, gamma_y_c1) = format_g2_point(gamma);
    println!("// Gamma (G2):");
    println!("gamma_X: [{}, {}]", gamma_x_c0, gamma_x_c1);
    println!("gamma_Y: [{}, {}]", gamma_y_c0, gamma_y_c1);
    println!();

    let (delta_x_c0, delta_x_c1, delta_y_c0, delta_y_c1) = format_g2_point(delta);
    println!("// Delta (G2):");
    println!("delta_X: [{}, {}]", delta_x_c0, delta_x_c1);
    println!("delta_Y: [{}, {}]", delta_y_c0, delta_y_c1);
    println!();

    println!("// Gamma_ABC (G1) - {} elements:", gamma_abc.len());
    for (i, point) in gamma_abc.iter().enumerate() {
        let (x, y) = format_g1_point(point);
        println!("gamma_abc[{}]: ({}, {})", i, x, y);
    }

    Ok(())
}

#[cfg(feature = "arkworks")]
fn format_g1_point(p: &ark_bn254::G1Affine) -> (String, String) {
    let x = format_field_element(&p.x);
    let y = format_field_element(&p.y);
    (x, y)
}

#[cfg(feature = "arkworks")]
fn format_g2_point(p: &ark_bn254::G2Affine) -> (String, String, String, String) {
    let x_c0 = format_field_element(&p.x.c0);
    let x_c1 = format_field_element(&p.x.c1);
    let y_c0 = format_field_element(&p.y.c0);
    let y_c1 = format_field_element(&p.y.c1);
    (x_c0, x_c1, y_c0, y_c1)
}

#[cfg(feature = "arkworks")]
fn format_field_element<F: ark_ff::PrimeField>(f: &F) -> String {
    let bytes = f.into_bigint().to_bytes_le();
    // Convert to uint256 (big-endian)
    let mut result = String::from("0x");
    for byte in bytes.iter().rev() {
        result.push_str(&format!("{:02x}", byte));
    }
    // Pad to 64 hex characters (32 bytes)
    while result.len() < 66 {
        result.insert_str(2, "00");
    }
    result
}

#[cfg(not(feature = "arkworks"))]
fn main() {
    eprintln!("Error: arkworks feature is required for this utility");
    std::process::exit(1);
}

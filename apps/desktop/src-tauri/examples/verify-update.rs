//! Release gate: verify the exact bytes, signed version and tamper rejection.
use base64::{Engine, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("Expected updater artifact")?;
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    let public = STANDARD.decode(
        config["plugins"]["updater"]["pubkey"]
            .as_str()
            .ok_or("Missing public key")?,
    )?;
    let key = PublicKey::decode(std::str::from_utf8(&public)?)?;
    let signature = STANDARD.decode(std::fs::read_to_string(format!("{path}.sig"))?.trim())?;
    let signature = Signature::decode(std::str::from_utf8(&signature)?)?;
    let mut bytes = std::fs::read(&path)?;
    key.verify(&bytes, &signature, true)?;
    let expected = format!("version:{}", env!("CARGO_PKG_VERSION"));
    if !signature
        .trusted_comment()
        .split_whitespace()
        .any(|part| part == expected)
    {
        return Err("Signature is not bound to the release version".into());
    }
    if bytes.is_empty() {
        return Err("Empty artifact".into());
    }
    bytes[0] ^= 1;
    if key.verify(&bytes, &signature, true).is_ok() {
        return Err("Tampered artifact was accepted".into());
    }
    println!("Verified update signature, version and tamper rejection: {path}");
    Ok(())
}

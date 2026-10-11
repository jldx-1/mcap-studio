use anyhow::{ensure, Context, Result};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(super) fn hash_password(password: &str) -> Result<String> {
    ensure!(
        (12..=256).contains(&password.len()),
        "Password must contain 12–256 bytes"
    );
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|e| anyhow::anyhow!(e))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| anyhow::anyhow!(e))
}

pub(super) fn verify_password(password: &str, encoded: &str) -> bool {
    password.len() <= 256
        && PasswordHash::new(encoded).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
}

pub(super) fn session_key(token: &str) -> Result<String> {
    ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Unauthenticated"
    );
    Ok(format!("{:x}", Sha256::digest(token.as_bytes())))
}

pub(super) fn id(value: &str) -> Result<()> {
    let parsed = Uuid::parse_str(value).context("Invalid ID")?;
    ensure!(parsed.to_string() == value, "Invalid ID");
    Ok(())
}

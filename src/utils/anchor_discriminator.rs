use sha2::{Digest, Sha256};

/// The discriminator is an 8-byte identifier derived from the first 8 bytes of the SHA256 hash of the string account:<AccountName>
pub fn derive_discriminator(account_name: &str) -> [u8; 8] {
    let discriminator_string = format!("account:{}", account_name);
    let hash = Sha256::digest(discriminator_string.as_bytes());
    let mut discriminator = [0u8; 8];
    discriminator.copy_from_slice(&hash[..8]);
    discriminator
}

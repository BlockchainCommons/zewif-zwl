use lazy_static::lazy_static;
use ring::hmac;

lazy_static! {
    static ref SECP256K1_SIGN_ONLY: secp256k1::Secp256k1<secp256k1::SignOnly> =
        secp256k1::Secp256k1::signing_only();
}

/// Random entropy, part of extended key.
type ChainCode = Vec<u8>;

const HARDENED_KEY_START_INDEX: u32 = 2_147_483_648; // 2 ** 31

/// KeyIndex indicates the key type and index of a child key.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum KeyIndex {
    /// Normal key, index range is from 0 to 2 ** 31 - 1
    Normal(u32),
    /// Hardened key, index range is from 2 ** 31 to 2 ** 32 - 1
    Hardened(u32),
}

impl KeyIndex {
    /// Check index range.
    pub fn is_valid(self) -> bool {
        match self {
            KeyIndex::Normal(i) => i < HARDENED_KEY_START_INDEX,
            KeyIndex::Hardened(i) => i >= HARDENED_KEY_START_INDEX,
        }
    }

    /// Generate Hardened KeyIndex from normalize index value.
    pub fn hardened_from_normalize_index(i: u32) -> Result<KeyIndex, secp256k1::Error> {
        if i < HARDENED_KEY_START_INDEX {
            Ok(KeyIndex::Hardened(HARDENED_KEY_START_INDEX + i))
        } else {
            Ok(KeyIndex::Hardened(i))
        }
    }

    /// Generate KeyIndex from raw index value.
    pub fn from_index(i: u32) -> Result<Self, secp256k1::Error> {
        if i < HARDENED_KEY_START_INDEX {
            Ok(KeyIndex::Normal(i))
        } else {
            Ok(KeyIndex::Hardened(i))
        }
    }
}

impl From<u32> for KeyIndex {
    fn from(index: u32) -> Self {
        KeyIndex::from_index(index).expect("KeyIndex")
    }
}

/// ExtendedPrivKey is used for child key derivation.
/// See [secp256k1 crate documentation](https://docs.rs/secp256k1) for SecretKey signatures usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedPrivKey {
    pub private_key: secp256k1::SecretKey,
    pub chain_code: ChainCode,
}

impl ExtendedPrivKey {
    /// Generate an ExtendedPrivKey from seed
    pub fn with_seed(seed: &[u8]) -> Result<ExtendedPrivKey, secp256k1::Error> {
        let signature = {
            let signing_key = hmac::Key::new(hmac::HMAC_SHA512, b"Bitcoin seed");
            let mut h = hmac::Context::with_key(&signing_key);
            h.update(&seed);
            h.sign()
        };
        let sig_bytes = signature.as_ref();
        let (key, chain_code) = sig_bytes.split_at(sig_bytes.len() / 2);
        let private_key = secp256k1::SecretKey::from_slice(key)?;
        Ok(ExtendedPrivKey {
            private_key,
            chain_code: chain_code.to_vec(),
        })
    }

    fn sign_hardended_key(&self, index: u32) -> hmac::Tag {
        let signing_key = hmac::Key::new(hmac::HMAC_SHA512, &self.chain_code);
        let mut h = hmac::Context::with_key(&signing_key);
        h.update(&[0x00]);
        h.update(&self.private_key[..]);
        h.update(&index.to_be_bytes());
        h.sign()
    }

    fn sign_normal_key(&self, index: u32) -> ring::hmac::Tag {
        let signing_key = hmac::Key::new(hmac::HMAC_SHA512, &self.chain_code);
        let mut h = hmac::Context::with_key(&signing_key);
        let public_key =
            secp256k1::PublicKey::from_secret_key(&SECP256K1_SIGN_ONLY, &self.private_key);
        h.update(&public_key.serialize());
        h.update(&index.to_be_bytes());
        h.sign()
    }

    /// Derive a child key from ExtendedPrivKey.
    pub fn derive_private_key(
        &self,
        key_index: KeyIndex,
    ) -> Result<ExtendedPrivKey, secp256k1::Error> {
        if !key_index.is_valid() {
            return Err(secp256k1::Error::InvalidTweak);
        }
        let signature = match key_index {
            KeyIndex::Hardened(index) => self.sign_hardended_key(index),
            KeyIndex::Normal(index) => self.sign_normal_key(index),
        };
        let sig_bytes = signature.as_ref();
        let (key, chain_code) = sig_bytes.split_at(sig_bytes.len() / 2);
        let private_key = secp256k1::SecretKey::from_slice(key)?;

        let tweak_scalar =
            secp256k1::Scalar::from_be_bytes(self.private_key.secret_bytes()).unwrap();

        let derived_priv_key = private_key.clone().add_tweak(&tweak_scalar).unwrap();

        Ok(ExtendedPrivKey {
            private_key: derived_priv_key,
            chain_code: chain_code.to_vec(),
        })
    }
}

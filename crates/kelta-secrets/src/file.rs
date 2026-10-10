//! `file:<name>` refs: tokens in one encrypted file in the data dir, for machines without a usable
//! keyring (SETTINGS §5). Layout: `MAGIC | salt (16) | nonce (24) | ciphertext`. The ciphertext is
//! XChaCha20-Poly1305 over a JSON `{name: token}` object with the 48-byte header as associated data,
//! under a key derived from the passphrase with Argon2id (19 MiB, t=2, p=1, pinned by `MAGIC`).
//! The key stays in memory (zeroized on drop) from unlock until the process exits; the passphrase
//! itself is wiped right after derivation.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Generate, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use kelta_proto::error::KeltaError;
use kelta_proto::secret::{Secret, SecretBackendStatus};
use parking_lot::Mutex;
use zeroize::{Zeroize, Zeroizing};

/// File format version: changing the layout or the KDF parameters needs a new magic.
const MAGIC: &[u8; 8] = b"KELTASF1";
const SALT: usize = 16;
const HEADER: usize = MAGIC.len() + SALT + 24;

struct Key {
    cipher: XChaCha20Poly1305,
    salt: [u8; SALT],
}

pub struct SecretFile {
    path: PathBuf,
    key: Mutex<Option<Key>>,
    /// Serializes read-modify-write cycles (lock order: `write` then `key`).
    write: Mutex<()>,
}

/// A decrypted `{name: token}` map, wiped on drop.
#[derive(Default)]
struct Plain(BTreeMap<String, String>);

impl Drop for Plain {
    fn drop(&mut self) {
        self.0.values_mut().for_each(Zeroize::zeroize);
    }
}

fn locked() -> KeltaError {
    KeltaError::needs_auth(
        "the encrypted secrets file is locked: enter its passphrase in Settings → Accounts",
    )
}

fn damaged() -> KeltaError {
    KeltaError::needs_auth("wrong passphrase, or the encrypted secrets file was modified")
}

fn io_error(what: &str, e: &std::io::Error) -> KeltaError {
    KeltaError::internal(format!("could not {what} the encrypted secrets file ({})", e.kind()))
}

fn derive(passphrase: &[u8], salt: &[u8; SALT]) -> Result<XChaCha20Poly1305, KeltaError> {
    let failed = |_| KeltaError::internal("key derivation failed");
    let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(failed)?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase, salt, key.as_mut())
        .map_err(failed)?;
    XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| KeltaError::internal("key derivation failed"))
}

fn salt_of(data: &[u8]) -> Result<[u8; SALT], KeltaError> {
    if data.len() < HEADER || &data[..MAGIC.len()] != MAGIC {
        return Err(damaged());
    }
    data[MAGIC.len()..MAGIC.len() + SALT].try_into().map_err(|_| damaged())
}

fn open(cipher: &XChaCha20Poly1305, data: &[u8]) -> Result<Plain, KeltaError> {
    salt_of(data)?;
    let (header, msg) = data.split_at(HEADER);
    let nonce = XNonce::try_from(&header[MAGIC.len() + SALT..]).map_err(|_| damaged())?;
    let plain = Zeroizing::new(cipher.decrypt(&nonce, Payload { msg, aad: header }).map_err(|_| damaged())?);
    serde_json::from_slice(&plain).map(Plain).map_err(|_| damaged())
}

fn seal(key: &Key, plain: &Plain) -> Result<Vec<u8>, KeltaError> {
    let nonce = XNonce::generate();
    let mut out = Vec::with_capacity(HEADER + 64);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&key.salt);
    out.extend_from_slice(&nonce);
    let json =
        Zeroizing::new(serde_json::to_vec(&plain.0).map_err(|_| KeltaError::internal("encoding failed"))?);
    let sealed = key
        .cipher
        .encrypt(&nonce, Payload { msg: &json, aad: &out })
        .map_err(|_| KeltaError::internal("encryption failed"))?;
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Write to a sibling temp file (created 0600 by `tempfile`), fsync, then rename over `path`:
/// readers see the old or the new file, never a torn one.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), KeltaError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| io_error("create the folder of", &e))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|e| io_error("write", &e))?;
    tmp.write_all(bytes).and_then(|()| tmp.as_file().sync_all()).map_err(|e| io_error("write", &e))?;
    tmp.persist(path).map_err(|e| io_error("replace", &e.error))?;
    Ok(())
}

impl SecretFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path, key: Mutex::new(None), write: Mutex::new(()) }
    }

    fn read(&self) -> Result<Option<Vec<u8>>, KeltaError> {
        match std::fs::read(&self.path) {
            Ok(d) => Ok(Some(d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error("read", &e)),
        }
    }

    /// Derive the key and check it against the file; with `create`, a missing file is created
    /// (empty) under this passphrase. Blocking: Argon2 takes ~19 MiB and tens of milliseconds.
    pub fn unlock(&self, passphrase: &[u8], create: bool) -> Result<(), KeltaError> {
        let _w = self.write.lock();
        let key = match self.read()? {
            Some(data) => {
                let salt = salt_of(&data)?;
                let key = Key { cipher: derive(passphrase, &salt)?, salt };
                open(&key.cipher, &data)?;
                key
            }
            None if create => {
                let salt = <[u8; SALT]>::generate();
                let key = Key { cipher: derive(passphrase, &salt)?, salt };
                replace(&self.path, &seal(&key, &Plain::default())?)?;
                key
            }
            None => {
                return Err(KeltaError::not_found(
                    "no encrypted secrets file yet: choose a passphrase to create it",
                ));
            }
        };
        *self.key.lock() = Some(key);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Secret, KeltaError> {
        let key = self.key.lock();
        let key = key.as_ref().ok_or_else(locked)?;
        let data =
            self.read()?.ok_or_else(|| KeltaError::needs_auth(format!("no token stored for file:{name}")))?;
        let plain = open(&key.cipher, &data)?;
        plain
            .0
            .get(name)
            .map(|v| Secret::new(v.as_str()))
            .ok_or_else(|| KeltaError::needs_auth(format!("no token stored for file:{name}")))
    }

    /// Store (`Some`) or remove (`None`) one token, atomically replacing the file.
    pub fn update(&self, name: &str, value: Option<&str>) -> Result<(), KeltaError> {
        let _w = self.write.lock();
        let key = self.key.lock();
        let key = key.as_ref().ok_or_else(locked)?;
        let mut plain = match self.read()? {
            Some(data) => open(&key.cipher, &data)?,
            None => Plain::default(),
        };
        let old = match value {
            Some(v) => plain.0.insert(name.to_owned(), v.to_owned()),
            None => plain.0.remove(name),
        };
        if let Some(mut old) = old {
            old.zeroize();
        }
        replace(&self.path, &seal(key, &plain)?)
    }

    pub fn status(&self) -> SecretBackendStatus {
        let (available, detail) = if self.key.lock().is_some() {
            (true, format!("unlocked ({})", self.path.display()))
        } else if self.path.exists() {
            (false, "locked: enter its passphrase in Settings → Accounts".to_owned())
        } else {
            (false, "not set up: choose a passphrase to create it".to_owned())
        };
        SecretBackendStatus { backend: "encrypted-file".into(), available, detail: Some(detail) }
    }
}

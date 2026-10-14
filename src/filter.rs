use crate::types::Type;
use regex::Regex;

/// Decides which keys are decoded. Keys that do not match are skipped
/// without decoding their value. All methods default to accepting everything.
pub trait Filter {
    fn matches_db(&self, _db: u32) -> bool {
        true
    }
    /// `enc_type` is the on-disk value type byte; see [`Type::from_encoding`].
    fn matches_type(&self, _enc_type: u8) -> bool {
        true
    }
    fn matches_key(&self, _key: &[u8]) -> bool {
        true
    }
}

/// Filter by database, logical [`Type`] and key regex. Each criterion is
/// ignored while empty; set criteria must all match.
#[derive(Default)]
pub struct Simple {
    databases: Vec<u32>,
    types: Vec<Type>,
    keys: Option<Regex>,
}

impl Simple {
    pub fn new() -> Simple {
        Simple::default()
    }

    /// Accept keys in `db`. Can be called multiple times.
    pub fn add_database(&mut self, db: u32) {
        self.databases.push(db);
    }

    /// Accept keys of type `typ`. Can be called multiple times.
    pub fn add_type(&mut self, typ: Type) {
        self.types.push(typ);
    }

    /// Accept keys matching `re` (keys are matched as lossy UTF-8). Replaces a previous regex.
    pub fn add_keys(&mut self, re: Regex) {
        self.keys = Some(re);
    }
}

impl Filter for Simple {
    fn matches_db(&self, db: u32) -> bool {
        if self.databases.is_empty() {
            true
        } else {
            self.databases.contains(&db)
        }
    }

    fn matches_type(&self, enc_type: u8) -> bool {
        if self.types.is_empty() {
            return true;
        }

        Type::from_encoding(enc_type).is_ok_and(|typ| self.types.contains(&typ))
    }

    fn matches_key(&self, key: &[u8]) -> bool {
        match &self.keys {
            None => true,
            Some(re) => {
                let key = String::from_utf8_lossy(key);
                re.is_match(&key)
            }
        }
    }
}

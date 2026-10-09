use crate::types::Type;
use regex::Regex;

pub trait Filter {
    fn matches_db(&self, _db: u32) -> bool {
        true
    }
    fn matches_type(&self, _enc_type: u8) -> bool {
        true
    }
    fn matches_key(&self, _key: &[u8]) -> bool {
        true
    }
}

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

    pub fn add_database(&mut self, db: u32) {
        self.databases.push(db);
    }

    pub fn add_type(&mut self, typ: Type) {
        self.types.push(typ);
    }

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

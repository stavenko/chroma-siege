use crate::components::Levels;
use serde::{Deserialize, Serialize};
use web_sys::Storage;

const KEY: &str = "chroma_siege_save_v1";

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct SaveData {
    pub coins: u64,
    pub perm: Levels,
    pub best_time: f32,
    pub runs: u32,
}

fn storage() -> Storage {
    web_sys::window()
        .expect("no window")
        .local_storage()
        .expect("access to localStorage threw")
        .expect("localStorage is not available")
}

pub fn load() -> SaveData {
    match storage().get_item(KEY).expect("localStorage.getItem failed") {
        None => SaveData::default(),
        Some(raw) => serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("corrupted save in localStorage['{KEY}']: {e}; raw = {raw}")),
    }
}

pub fn store(data: &SaveData) {
    let raw = serde_json::to_string(data).expect("save serialization failed");
    storage().set_item(KEY, &raw).expect("localStorage.setItem failed");
}

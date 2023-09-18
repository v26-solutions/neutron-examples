use cosmwasm_std::Storage;

pub fn set_connection_id(storage: &mut dyn Storage, connection_id: &str) {
    storage.set(b"connection_id", connection_id.as_bytes());
}

pub fn connection_id(storage: &dyn Storage) -> String {
    storage
        .get(b"connection_id")
        .map(String::from_utf8)
        .expect("connection id set during instantiation")
        .expect("a valid utf-8 sequence of bytes")
}

pub fn set_balance_icq_denom(storage: &mut dyn Storage, balance_icq_denom: &str) {
    storage.set(b"balance_icq_denom", balance_icq_denom.as_bytes());
}

pub fn balance_icq_denom(storage: &dyn Storage) -> String {
    storage
        .get(b"balance_icq_denom")
        .map(String::from_utf8)
        .expect("balance icq denom set during instantiation")
        .expect("a valid utf-8 sequence of bytes")
}

pub fn set_ica_set_size(storage: &mut dyn Storage, ica_set_size: u32) {
    storage.set(b"ica_set_size", &ica_set_size.to_be_bytes());
}

pub fn ica_set_size(storage: &dyn Storage) -> u32 {
    storage
        .get(b"ica_set_size")
        .expect("ica set size set during instantiation")
        .try_into()
        .map(u32::from_be_bytes)
        .expect("a vector of 4 bytes")
}

pub fn set_icq_update_period(storage: &mut dyn Storage, icq_update_period: u64) {
    storage.set(b"icq_update_period", &icq_update_period.to_be_bytes());
}

pub fn icq_update_period(storage: &dyn Storage) -> u64 {
    storage
        .get(b"icq_update_period")
        .expect("icq update period set during instantiation")
        .try_into()
        .map(u64::from_be_bytes)
        .expect("a vector of 8 bytes")
}

fn ica_map_key(prefix: &[u8], ica_idx: u32) -> Vec<u8> {
    [prefix, ica_idx.to_be_bytes().as_slice()].concat()
}

fn ica_icq_id_key(ica_idx: u32) -> Vec<u8> {
    ica_map_key(b"ica_icq_id".as_slice(), ica_idx)
}

fn icq_ica_idx_key(icq_id: u64) -> Vec<u8> {
    [b"icq_ica_idx".as_slice(), icq_id.to_be_bytes().as_slice()].concat()
}

// maps ica_idx => icq_id & icq_id => ica_idx
pub fn set_ica_icq_id(storage: &mut dyn Storage, ica_idx: u32, icq_id: u64) {
    storage.set(&ica_icq_id_key(ica_idx), &icq_id.to_be_bytes());
    storage.set(&icq_ica_idx_key(icq_id), &ica_idx.to_be_bytes());
}

pub fn ica_icq_id(storage: &dyn Storage, ica_idx: u32) -> Option<u64> {
    storage
        .get(&ica_icq_id_key(ica_idx))?
        .try_into()
        .map(u64::from_be_bytes)
        .map(Some)
        .expect("a vector of 8 bytes")
}

pub fn icq_ica_idx(storage: &dyn Storage, icq_id: u64) -> Option<u32> {
    storage
        .get(&icq_ica_idx_key(icq_id))?
        .try_into()
        .map(u32::from_be_bytes)
        .map(Some)
        .expect("a vector of 4 bytes")
}

fn ica_addr_key(ica_idx: u32) -> Vec<u8> {
    ica_map_key(b"ica_addr".as_slice(), ica_idx)
}

pub fn set_ica_addr(storage: &mut dyn Storage, ica_idx: u32, addr: &str) {
    storage.set(&ica_addr_key(ica_idx), addr.as_bytes());
}

pub fn ica_addr(storage: &dyn Storage, ica_idx: u32) -> Option<String> {
    storage
        .get(&ica_addr_key(ica_idx))
        .map(String::from_utf8)
        .transpose()
        .expect("a valid utf-8 sequence of bytes")
}

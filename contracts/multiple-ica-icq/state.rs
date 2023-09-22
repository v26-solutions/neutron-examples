macro_rules! item_key {
    ($i:ident) => {
        concat!(module_path!(), "::", stringify!($i)).as_bytes()
    };
}

macro_rules! map_key {
    ($k:ident, $v:ident) => {
        format!(
            "{}.{}",
            concat!(module_path!(), "::", stringify!($k), ":", stringify!($v)),
            $k
        )
        .as_bytes()
    };
}

macro_rules! init_config {
    ($i:ident: String) => {
        ::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: &str) {
                storage.set(item_key!($i), $i.as_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> String {
                storage
                    .get(item_key!($i))
                    .map(String::from_utf8)
                    .expect(concat!(stringify!($i), " set during instantiation"))
                    .expect("a valid utf-8 sequence of bytes")
            }
        }
    };

    ($i:ident: $t:ty) => {
        ::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: $t) {
                storage.set(item_key!($i), &$i.to_be_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> $t {
                storage
                    .get(item_key!($i))
                    .expect(concat!(stringify!($i), " set during instantiation"))
                    .try_into()
                    .map($t::from_be_bytes)
                    .expect(concat!("the exact amount of bytes in a ", stringify!($t)))
            }
        }
    };
}

macro_rules! map {
    ($k:ident:$kt:ty => $v:ident: String) => {
        ::paste::paste! {
            pub fn [<set _ $k _ $v>](storage: &mut dyn ::cosmwasm_std::Storage, $k: $kt, $v: &str) {
                storage.set(map_key!($k, $v), $v.as_bytes())
            }

            pub fn [<$k _ $v>](storage: &dyn ::cosmwasm_std::Storage, $k: $kt) -> Option<String> {
                storage
                    .get(map_key!($k, $v))
                    .map(String::from_utf8)
                    .transpose()
                    .expect("a valid utf-8 sequence of bytes")
            }
        }
    };

    ($k:ident:$kt:ty => $v:ident: $int_type:ty) => {
        ::paste::paste! {
            pub fn [<set _ $k _ $v>](storage: &mut dyn ::cosmwasm_std::Storage, $k: $kt, $v: $int_type) {
                storage.set(map_key!($k, $v), $v.to_be_bytes().as_slice())
            }

            pub fn [<$k _ $v>](storage: &dyn ::cosmwasm_std::Storage, $k: $kt) -> Option<$int_type> {
                storage
                    .get(map_key!($k, $v))?
                    .try_into()
                    .map($int_type::from_be_bytes)
                    .map(Some)
                    .expect(concat!("the exact amount of bytes in a ", stringify!($int_type)))
            }
        }
    };
}

init_config!(delegations_icq_validator : String);
init_config!(connection_id             : String);
init_config!(balance_icq_denom         : String);
init_config!(ica_set_size              : u32);
init_config!(icq_update_period         : u64);

map!(ica: u32 => addr               : String);
map!(icq: u64 => ica_idx            : u32);
map!(icq: u64 => kind               : u32);
map!(ica: u32 => balance_icq_id     : u64);
map!(ica: u32 => delegations_icq_id : u64);

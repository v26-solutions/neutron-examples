#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use cosmwasm_std::{from_binary, Binary, Coin, CustomQuery, Deps, QueryRequest, Reply, StdError};
use neutron_sdk::{
    bindings::{query::NeutronQuery, types::RegisteredQuery},
    interchain_queries::{
        check_query_type, get_registered_query, queries::get_raw_interchain_query_result,
        types::QueryType,
    },
    NeutronError,
};
pub use paste;
use prost::Message;

#[macro_export]
macro_rules! item_key {
    ($i:ident) => {
        concat!(module_path!(), "::", stringify!($i)).as_bytes()
    };
}

#[macro_export]
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

#[macro_export]
macro_rules! init_config {
    ($i:ident: String) => {
        $crate::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: &str) {
                storage.set($crate::item_key!($i), $i.as_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> String {
                storage
                    .get($crate::item_key!($i))
                    .map(String::from_utf8)
                    .expect(concat!(stringify!($i), " set during instantiation"))
                    .expect("a valid utf-8 sequence of bytes")
            }
        }
    };

    ($i:ident: $t:ty) => {
        $crate::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: $t) {
                storage.set($crate::item_key!($i), &$i.to_be_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> $t {
                storage
                    .get($crate::item_key!($i))
                    .expect(concat!(stringify!($i), " set during instantiation"))
                    .try_into()
                    .map($t::from_be_bytes)
                    .expect(concat!("the exact amount of bytes in a ", stringify!($t)))
            }
        }
    };
}

#[macro_export]
macro_rules! item {
    ($i:ident: String) => {
        $crate::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: &str) {
                storage.set($crate::item_key!($i), $i.as_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> Option<String> {
                storage
                    .get($crate::item_key!($i))
                    .map(String::from_utf8)
                    .transpose()
                    .expect("a valid utf-8 sequence of bytes")
            }
        }
    };

    ($i:ident: $t:ty) => {
        $crate::paste::paste! {
            pub fn [<set _ $i>](storage: &mut dyn ::cosmwasm_std::Storage, $i: $t) {
                storage.set($crate::item_key!($i), &$i.to_be_bytes())
            }

            pub fn $i(storage: &dyn ::cosmwasm_std::Storage) -> Option<$t> {
                let int = storage
                    .get($crate::item_key!($i))?
                    .try_into()
                    .map($t::from_be_bytes)
                    .expect(concat!("the exact amount of bytes in a ", stringify!($t)));

                Some(int)
            }
        }
    };
}

#[macro_export]
macro_rules! map {
    ($k:ident:$kt:ty => $v:ident: String) => {
        $crate::paste::paste! {
            pub fn [<set _ $k _ $v>](storage: &mut dyn ::cosmwasm_std::Storage, $k: $kt, $v: &str) {
                storage.set($crate::map_key!($k, $v), $v.as_bytes())
            }

            pub fn [<$k _ $v>](storage: &dyn ::cosmwasm_std::Storage, $k: $kt) -> Option<String> {
                storage
                    .get($crate::map_key!($k, $v))
                    .map(String::from_utf8)
                    .transpose()
                    .expect("a valid utf-8 sequence of bytes")
            }
        }
    };

    ($k:ident:$kt:ty => $v:ident: $int_type:ty) => {
        $crate::paste::paste! {
            pub fn [<set _ $k _ $v>](storage: &mut dyn ::cosmwasm_std::Storage, $k: $kt, $v: $int_type) {
                storage.set($crate::map_key!($k, $v), $v.to_be_bytes().as_slice())
            }

            pub fn [<$k _ $v>](storage: &dyn ::cosmwasm_std::Storage, $k: $kt) -> Option<$int_type> {
                storage
                    .get($crate::map_key!($k, $v))?
                    .try_into()
                    .map($int_type::from_be_bytes)
                    .map(Some)
                    .expect(concat!("the exact amount of bytes in a ", stringify!($int_type)))
            }
        }
    };
}

#[macro_export]
macro_rules! debug {
    ($deps:ident, $($arg:tt)*) => {
        $deps.api.debug(&format!("{}: {}", env!("CARGO_PKG_NAME"), format!($($arg)*)))
    };
}

#[must_use]
pub fn combine_u32s(a: u32, b: u32) -> u64 {
    (u64::from(a) << 32) | u64::from(b)
}

#[must_use]
pub fn split_u64(x: u64) -> (u32, u32) {
    let a = u32::try_from(x >> 32).unwrap();
    let b = u32::try_from(x.rotate_left(32) >> 32).unwrap();
    (a, b)
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenAckVersion {
    pub version: String,
    pub controller_connection_id: String,
    pub host_connection_id: String,
    pub address: String,
    pub encoding: String,
    pub tx_type: String,
}

#[must_use]
pub fn ica_idx_from_port_id(port_id: &str) -> Option<u32> {
    port_id.split('.').last().and_then(|s| s.parse().ok())
}

#[derive(Debug, thiserror::Error)]
pub enum ParseReplyError {
    #[error("{0}")]
    SubMsgFailure(String),
    #[error("reply data missing")]
    ReplyDataMissing,
    #[error(transparent)]
    CosmwasmStd(#[from] cosmwasm_std::StdError),
}

/// Tries to parse the query id of a newly registered ICQ from the reply data
pub fn parse_icq_registration_reply(reply: Reply) -> Result<u64, ParseReplyError> {
    #[cosmwasm_schema::cw_serde]
    struct MsgRegisterInterchainQueryResponse {
        id: u64,
    }

    let res = reply
        .result
        .into_result()
        .map_err(ParseReplyError::SubMsgFailure)?;

    let data = res.data.ok_or(ParseReplyError::ReplyDataMissing)?;

    let msg: MsgRegisterInterchainQueryResponse = from_binary(&data)?;

    Ok(msg.id)
}

/// Tries to parse the sequence number and channel id of a newly issued IBC tx from the reply data
pub fn parse_issue_tx_reply(reply: Reply) -> Result<(u64, String), ParseReplyError> {
    #[cosmwasm_schema::cw_serde]
    struct MsgSubmitTxResponse {
        sequence_id: u64,
        channel: String,
    }

    let res = reply
        .result
        .into_result()
        .map_err(ParseReplyError::SubMsgFailure)?;

    let data = res.data.ok_or(ParseReplyError::ReplyDataMissing)?;

    let MsgSubmitTxResponse {
        sequence_id,
        channel,
    } = from_binary(&data)?;

    Ok((sequence_id, channel))
}

pub fn icq_deposit_fee(deps: Deps<impl CustomQuery>) -> Result<Coin, StdError> {
    #[cosmwasm_schema::cw_serde]
    struct Params {
        query_submit_timeout: String,
        query_deposit: Vec<Coin>,
        tx_query_removal_limit: String,
    }

    #[cosmwasm_schema::cw_serde]
    struct QueryParamsResponse {
        params: Params,
    }

    let res: QueryParamsResponse = deps.querier.query(&QueryRequest::Stargate {
        path: "/neutron.interchainqueries.Query/Params".to_owned(),
        data: Binary(vec![]),
    })?;

    let coin = res
        .params
        .query_deposit
        .into_iter()
        .next()
        .expect("there should always be a deposit coin");

    Ok(coin)
}

#[derive(Debug, thiserror::Error)]
pub enum QueryBalanceIcqError {
    #[error(transparent)]
    CosmwasmStd(#[from] cosmwasm_std::StdError),
    #[error(transparent)]
    NeutronSdk(#[from] NeutronError),
    #[error(transparent)]
    Protobuf(#[from] prost::DecodeError),
}

#[derive(Debug, Clone)]
pub struct RemoteBalance {
    pub last_submitted_result_local_height: u64,
    pub balance: Option<Coin>,
}

pub fn updated_registered_kv_query(
    deps: Deps<NeutronQuery>,
    query_id: u64,
) -> Result<Option<RegisteredQuery>, NeutronError> {
    let res = get_registered_query(deps, query_id)?;

    let registered_query = res.registered_query;

    let last_submitted_local_height = registered_query.last_submitted_result_local_height;

    if last_submitted_local_height == 0 {
        return Ok(None);
    }

    check_query_type(registered_query.query_type, QueryType::KV)?;

    Ok(Some(registered_query))
}

pub fn query_balance_icq(
    deps: Deps<NeutronQuery>,
    query_id: u64,
) -> Result<Option<RemoteBalance>, QueryBalanceIcqError> {
    #[derive(Clone, PartialEq, Message)]
    struct RawCoin {
        #[prost(string, tag = "1")]
        pub denom: String,
        #[prost(string, tag = "2")]
        pub amount: String,
    }

    let Some(registered_query) = updated_registered_kv_query(deps, query_id)? else {
        return Ok(None);
    };

    let registered_query_result = get_raw_interchain_query_result(deps, query_id)?;

    assert_eq!(
        registered_query_result.result.kv_results.len(),
        1,
        "only a single balance key requested means exactly one storage entry submitted"
    );

    let storage_entry = registered_query_result.result.kv_results.first().unwrap();

    let RawCoin { denom, amount } = RawCoin::decode(storage_entry.value.as_slice())?;

    let last_submitted_result_local_height = registered_query.last_submitted_result_local_height;

    if denom.is_empty() && amount.is_empty() {
        return Ok(Some(RemoteBalance {
            last_submitted_result_local_height,
            balance: None,
        }));
    }

    let amount = amount.parse()?;

    Ok(Some(RemoteBalance {
        last_submitted_result_local_height,
        balance: Some(Coin { denom, amount }),
    }))
}

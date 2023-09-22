use cosmwasm_std::{
    from_binary, Binary, Coin, Deps, QuerierWrapper, QueryRequest, Reply, StdError,
};
use neutron_sdk::{
    bindings::{query::NeutronQuery, types::RegisteredQuery},
    interchain_queries::{
        check_query_type, get_registered_query, queries::get_raw_interchain_query_result,
        query_kv_result, types::QueryType, v045::types::Delegations,
    },
    NeutronError,
};
use prost::Message;

use crate::msgs::IcaLastDelegation;

macro_rules! debug {
    ($deps:ident, $($arg:tt)*) => {
        $deps.api.debug(&format!("{}: {}", env!("CARGO_PKG_NAME"), format!($($arg)*)))
    };
}

pub(crate) use debug;

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
    CosmwasmStd(#[from] StdError),
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

pub fn icq_deposit_fee(querier: &QuerierWrapper) -> Result<Coin, StdError> {
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

    let res: QueryParamsResponse = querier.query(&QueryRequest::Stargate {
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

fn updated_registered_kv_query(
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

pub fn query_delegation_icq(
    deps: Deps<NeutronQuery>,
    query_id: u64,
) -> Result<Option<IcaLastDelegation>, NeutronError> {
    let Some(registered_query) = updated_registered_kv_query(deps, query_id)? else {
        return Ok(None);
    };

    let delegations: Delegations = query_kv_result(deps, query_id)?;

    assert!(
        delegations.delegations.len() < 2,
        "only one validator is ever delegated to"
    );

    let delegation = delegations.delegations.into_iter().next();

    let last_submitted_result_local_height = registered_query.last_submitted_result_local_height;

    Ok(Some(IcaLastDelegation {
        delegation,
        last_submitted_result_local_height,
    }))
}

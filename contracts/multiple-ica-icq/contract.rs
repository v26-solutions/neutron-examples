#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value
)]

pub mod helper;
pub mod msgs;

use cosmwasm_std::{
    entry_point, from_slice, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response,
    SubMsg,
};
use msgs::IcaLastDelegationResponse;
use neutron_sdk::{
    bindings::{msg::NeutronMsg, query::NeutronQuery},
    interchain_queries::v045::{
        new_register_balance_query_msg, new_register_delegator_delegations_query_msg,
    },
    sudo::msg::SudoMsg,
};

use crate::msgs::{
    ExecuteMsg, IcaLastBalance, IcaLastBalanceResponse, IcaMetadata, IcaMetadataResponse,
    IcaSetSizeResponse, InstantiateMsg, QueryMsg,
};

use common::{
    combine_u32s, debug, ica_idx_from_port_id, icq_deposit_fee, parse_icq_registration_reply,
    query_balance_icq, split_u64, OpenAckVersion, RemoteBalance,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    CosmwasmStd(#[from] cosmwasm_std::StdError),
    #[error(transparent)]
    NeutronSdk(#[from] neutron_sdk::NeutronError),
    #[error(transparent)]
    ParseReply(#[from] common::ParseReplyError),
    #[error(transparent)]
    QueryBalanceIcq(#[from] common::QueryBalanceIcqError),
    #[error("ica index {ica_idx} is out of bounds, ica set size is {ica_set_size}")]
    IcaIndexOutOfBounds { ica_idx: u32, ica_set_size: u32 },
    #[error("icq deposit missing")]
    IcqDepositMissing,
    #[error("incorrect icq deposit asset")]
    IncorrectIcqDepositAsset,
    #[error("insufficient icq deposit")]
    InsufficientIcqDeposit,
}

const BALANCE_ICQ_KIND: u32 = 1;
const DELEGATIONS_ICQ_KIND: u32 = 2;

pub mod state {
    use common::{init_config, map};

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
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "handling instantiate msg");

    // save configuration
    state::set_connection_id(deps.storage, &msg.connection_id);

    state::set_ica_set_size(deps.storage, msg.ica_set_size);

    state::set_icq_update_period(deps.storage, msg.icq_update_period);

    state::set_balance_icq_denom(deps.storage, &msg.balance_icq_denom);

    state::set_delegations_icq_validator(deps.storage, &msg.delegations_icq_validator);

    // get required ICQ deposit fee
    let icq_deposit_fee = icq_deposit_fee(deps.as_ref())?;

    // check instantiator has provided the required funds for an ICQ per ICA
    let deposit = info.funds.first().ok_or(Error::IcqDepositMissing)?;

    if deposit.denom != icq_deposit_fee.denom {
        return Err(Error::IncorrectIcqDepositAsset);
    }

    let number_of_icqs = msg.ica_set_size * 2;

    let required_deposit_amount = icq_deposit_fee.amount.u128() * u128::from(number_of_icqs);

    if deposit.amount.u128() < required_deposit_amount {
        return Err(Error::InsufficientIcqDeposit);
    }

    // Generate ICA registration messages
    let register_ica_msgs =
        (0..msg.ica_set_size).map(|idx| NeutronMsg::RegisterInterchainAccount {
            connection_id: msg.connection_id.clone(),
            interchain_account_id: idx.to_string(),
        });

    Ok(Response::default().add_messages(register_ica_msgs))
}

#[entry_point]
pub fn execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: ExecuteMsg,
) -> Result<Response, Error> {
    Ok(Response::default())
}

pub fn sudo_open_ack(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    port_id: String,
    _channel_id: String,
    _counterparty_channel_id: String,
    counterparty_version: String,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(
        deps,
        "received open ack for {port_id}: {counterparty_version}"
    );

    // The version variable contains a JSON value with multiple fields,
    // including the generated account address.
    let parsed_version: OpenAckVersion =
        from_slice(counterparty_version.as_bytes()).expect("valid counterparty_version");

    let ica_idx = ica_idx_from_port_id(&port_id).expect("valid port id");

    state::set_ica_addr(deps.storage, ica_idx, &parsed_version.address);

    let connection_id = state::connection_id(deps.storage);

    let icq_update_period = state::icq_update_period(deps.storage);

    let balance_icq_denom = state::balance_icq_denom(deps.storage);

    let delegations_icq_validator = state::delegations_icq_validator(deps.storage);

    let balance_icq_register_msg = new_register_balance_query_msg(
        connection_id.clone(),
        parsed_version.address.clone(),
        balance_icq_denom,
        icq_update_period,
    )?;

    let delegations_icq_register_msg = new_register_delegator_delegations_query_msg(
        connection_id,
        parsed_version.address,
        vec![delegations_icq_validator],
        icq_update_period,
    )?;

    let response = Response::default()
        .add_submessage(SubMsg::reply_on_success(
            balance_icq_register_msg,
            combine_u32s(BALANCE_ICQ_KIND, ica_idx),
        ))
        .add_submessage(SubMsg::reply_on_success(
            delegations_icq_register_msg,
            combine_u32s(DELEGATIONS_ICQ_KIND, ica_idx),
        ));

    Ok(response)
}

pub fn sudo_kv_query_result(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    query_id: u64,
) -> Result<Response<NeutronMsg>, Error> {
    let ica_idx =
        state::icq_ica_idx(deps.storage, query_id).expect("the icq is associated with an ica");

    let ica_addr = state::ica_addr(deps.storage, ica_idx).expect("the ica has an address");

    let ica_kind = state::icq_kind(deps.storage, query_id).expect("the icq has a kind");

    let kind_str = match ica_kind {
        BALANCE_ICQ_KIND => stringify!(BALANCE_ICQ_KIND),
        DELEGATIONS_ICQ_KIND => stringify!(DELEGATIONS_ICQ_KIND),
        _ => unreachable!(),
    };

    debug!(
        deps,
        "received {kind_str} ICQ {query_id} update for ICA {ica_idx} with address: {ica_addr}"
    );

    Ok(Response::default())
}

#[entry_point]
pub fn sudo(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    msg: SudoMsg,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "handling sudo msg");

    match msg {
        SudoMsg::OpenAck {
            port_id,
            channel_id,
            counterparty_channel_id,
            counterparty_version,
        } => sudo_open_ack(
            deps,
            env,
            port_id,
            channel_id,
            counterparty_channel_id,
            counterparty_version,
        ),

        SudoMsg::KVQueryResult { query_id } => sudo_kv_query_result(deps, env, query_id),

        _ => {
            debug!(deps, "unexpected sudo msg: {msg:?}");
            Ok(Response::default())
        }
    }
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> Result<Response, Error> {
    let reply_id = reply.id;

    let (icq_kind, ica_idx) = split_u64(reply_id);

    debug!(
        deps,
        "received reply with id {}, split into ICQ kind {icq_kind} and ICA index {ica_idx}",
        reply.id
    );

    let icq_id = parse_icq_registration_reply(reply)?;

    state::set_icq_ica_idx(deps.storage, icq_id, ica_idx);

    state::set_icq_kind(deps.storage, icq_id, icq_kind);

    match icq_kind {
        BALANCE_ICQ_KIND => {
            debug!(deps, "Got balance ICQ with id {icq_id} for ICA {ica_idx}");
            state::set_ica_balance_icq_id(deps.storage, ica_idx, icq_id);
        }

        DELEGATIONS_ICQ_KIND => {
            debug!(
                deps,
                "Got delegations ICQ with id {icq_id} for ICA {ica_idx}"
            );
            state::set_ica_delegations_icq_id(deps.storage, ica_idx, icq_id);
        }

        _ => {
            debug!(
                deps,
                "received reply with id {reply_id} that has unknown ICQ kind: {icq_kind}",
            );
        }
    };

    Ok(Response::default())
}

pub fn ica_idx_in_bounds(deps: Deps<NeutronQuery>, ica_idx: u32) -> Result<bool, Error> {
    let ica_set_size = state::ica_set_size(deps.storage);

    if ica_idx >= ica_set_size {
        return Err(Error::IcaIndexOutOfBounds {
            ica_idx,
            ica_set_size,
        });
    }

    Ok(true)
}

pub fn query_ica_metadata(
    deps: Deps<NeutronQuery>,
    ica_idx: u32,
) -> Result<IcaMetadataResponse, Error> {
    ica_idx_in_bounds(deps, ica_idx)?;

    let maybe_ica_addr = state::ica_addr(deps.storage, ica_idx);

    let maybe_balance_icq_id = state::ica_balance_icq_id(deps.storage, ica_idx);

    let maybe_delegations_icq_id = state::ica_delegations_icq_id(deps.storage, ica_idx);

    let metadata = maybe_ica_addr
        .zip(maybe_balance_icq_id)
        .zip(maybe_delegations_icq_id)
        .map(
            |((address, balance_icq_id), delegations_icq_id)| IcaMetadata {
                address,
                balance_icq_id,
                delegation_icq_id: delegations_icq_id,
            },
        );

    Ok(IcaMetadataResponse { metadata })
}

pub fn query_last_ica_balance(
    deps: Deps<NeutronQuery>,
    ica_idx: u32,
) -> Result<IcaLastBalanceResponse, Error> {
    ica_idx_in_bounds(deps, ica_idx)?;

    let Some(icq_id) = state::ica_balance_icq_id(deps.storage, ica_idx) else {
        return Ok(IcaLastBalanceResponse::default());
    };

    debug!(deps, "querying balance ICQ {icq_id} for ICA {ica_idx}");

    let Some(RemoteBalance {
        last_submitted_result_local_height,
        balance,
    }) = query_balance_icq(deps, icq_id)?
    else {
        return Ok(IcaLastBalanceResponse::default());
    };

    let address =
        state::ica_addr(deps.storage, ica_idx).expect("a registered ica has an address set");

    let last_balance = IcaLastBalance {
        balance,
        address,
        last_submitted_result_local_height,
    };

    Ok(IcaLastBalanceResponse {
        last_balance: Some(last_balance),
    })
}

pub fn query_last_ica_delegation(
    deps: Deps<NeutronQuery>,
    ica_idx: u32,
) -> Result<IcaLastDelegationResponse, Error> {
    ica_idx_in_bounds(deps, ica_idx)?;

    let Some(icq_id) = state::ica_delegations_icq_id(deps.storage, ica_idx) else {
        return Ok(IcaLastDelegationResponse::default());
    };

    debug!(deps, "querying delegation ICQ {icq_id} for ICA {ica_idx}");

    let last_delegation = helper::query_delegation_icq(deps, icq_id)?;

    Ok(IcaLastDelegationResponse { last_delegation })
}

#[entry_point]
pub fn query(deps: Deps<NeutronQuery>, _env: Env, msg: QueryMsg) -> Result<Binary, Error> {
    let res = match msg {
        QueryMsg::IcaSetSize {} => {
            let ica_set_size = state::ica_set_size(deps.storage);

            to_binary(&IcaSetSizeResponse { ica_set_size })?
        }

        QueryMsg::IcaMetadata { ica_idx } => {
            let ica_metadata = query_ica_metadata(deps, ica_idx)?;

            to_binary(&ica_metadata)?
        }

        QueryMsg::IcaLastBalance { ica_idx } => {
            let last_ica_balance = query_last_ica_balance(deps, ica_idx)?;

            to_binary(&last_ica_balance)?
        }

        QueryMsg::IcaLastDelegation { ica_idx } => {
            let last_ica_delegation = query_last_ica_delegation(deps, ica_idx)?;

            to_binary(&last_ica_delegation)?
        }
    };

    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn icq_reply_id_round_trip() {
        for i in 0..100 {
            assert_eq!(
                (BALANCE_ICQ_KIND, i),
                split_u64(combine_u32s(BALANCE_ICQ_KIND, i))
            );
            assert_eq!(
                (DELEGATIONS_ICQ_KIND, i),
                split_u64(combine_u32s(DELEGATIONS_ICQ_KIND, i))
            );
        }
    }
}

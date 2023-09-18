#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value
)]

pub mod helper;
pub mod msgs;
pub mod state;

use cosmwasm_std::{
    entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, SubMsg,
};
use neutron_sdk::{
    bindings::{msg::NeutronMsg, query::NeutronQuery},
    interchain_queries::v045::new_register_balance_query_msg,
    sudo::msg::SudoMsg,
};

use crate::{
    helper::{debug, RemoteBalance},
    msgs::{
        ExecuteMsg, IcaLastBalanceResponse, IcaMetadata, IcaMetadataResponse, IcaSetSizeResponse,
        InstantiateMsg, OpenAckVersion, QueryMsg,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    CosmwasmStd(#[from] cosmwasm_std::StdError),
    #[error(transparent)]
    NeutronSdk(#[from] neutron_sdk::NeutronError),
    #[error(transparent)]
    ParseReply(#[from] helper::ParseReplyError),
    #[error(transparent)]
    QueryBalanceIcq(#[from] helper::QueryBalanceIcqError),
    #[error("ica index {ica_idx} is out of bounds, ica set size is {ica_set_size}")]
    IcaIndexOutOfBounds { ica_idx: u32, ica_set_size: u32 },
    #[error("icq deposit missing")]
    IcqDepositMissing,
    #[error("incorrect icq deposit asset")]
    IncorrectIcqDepositAsset,
    #[error("insufficient icq deposit")]
    InsufficientIcqDeposit,
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

    state::set_balance_icq_denom(deps.storage, &msg.balance_icq_denom);

    state::set_ica_set_size(deps.storage, msg.ica_set_size);

    state::set_icq_update_period(deps.storage, msg.icq_update_period);

    // get required ICQ deposit fee
    let icq_deposit_fee = helper::icq_deposit_fee(&deps.querier)?;

    // check instantiator has provided the required funds for an ICQ per ICA
    let deposit = info.funds.first().ok_or(Error::IcqDepositMissing)?;

    if deposit.denom != icq_deposit_fee.denom {
        return Err(Error::IncorrectIcqDepositAsset);
    }

    let required_deposit_amount = icq_deposit_fee.amount.u128() * u128::from(msg.ica_set_size);

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
    let parsed_version: OpenAckVersion = serde_json_wasm::from_str(counterparty_version.as_str())
        .expect("valid counterparty_version");

    let ica_idx = helper::ica_idx_from_port_id(&port_id).expect("valid port id");

    state::set_ica_addr(deps.storage, ica_idx, &parsed_version.address);

    let connection_id = state::connection_id(deps.storage);

    let balance_icq_denom = state::balance_icq_denom(deps.storage);

    let icq_update_period = state::icq_update_period(deps.storage);

    let icq_register_msg = new_register_balance_query_msg(
        connection_id,
        parsed_version.address,
        balance_icq_denom,
        icq_update_period,
    )?;

    Ok(Response::default()
        .add_submessage(SubMsg::reply_on_success(icq_register_msg, ica_idx.into())))
}

pub fn sudo_kv_query_result(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    query_id: u64,
) -> Result<Response<NeutronMsg>, Error> {
    let ica_idx =
        state::icq_ica_idx(deps.storage, query_id).expect("the icq is associated with an ica");

    let ica_addr = state::ica_addr(deps.storage, ica_idx).expect("the ica has an address");

    debug!(
        deps,
        "received received query {query_id} update for ica: {ica_idx} => {ica_addr}"
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
    debug!(deps, "received reply with id: {}", reply.id);

    let ica_idx: u32 = reply.id.try_into().expect("reply id is a u32 ica index");

    let icq_id = helper::parse_icq_registration_reply(reply)?;

    debug!(deps, "recieved icq id for ica {}: {}", ica_idx, icq_id);

    state::set_ica_icq_id(deps.storage, ica_idx, icq_id);

    Ok(Response::default())
}

pub fn query_last_ica_balance(
    deps: Deps<NeutronQuery>,
    _env: Env,
    ica_idx: u32,
) -> Result<IcaLastBalanceResponse, Error> {
    let ica_set_size = state::ica_set_size(deps.storage);

    if ica_idx >= ica_set_size {
        return Err(Error::IcaIndexOutOfBounds {
            ica_idx,
            ica_set_size,
        });
    }

    let address = state::ica_addr(deps.storage, ica_idx);

    let Some(icq_id) = state::ica_icq_id(deps.storage, ica_idx) else {
        return Ok(IcaLastBalanceResponse {
            address,
            ..Default::default()
        });
    };

    debug!(deps, "querying balance icq with id: {icq_id}");

    let Some(RemoteBalance {
        last_submitted_local_height,
        balance,
    }) = helper::query_balance_icq(deps, icq_id)?
    else {
        return Ok(IcaLastBalanceResponse {
            address,
            ..Default::default()
        });
    };

    Ok(IcaLastBalanceResponse {
        address,
        balance,
        last_local_update_height: Some(last_submitted_local_height),
    })
}

#[entry_point]
pub fn query(deps: Deps<NeutronQuery>, env: Env, msg: QueryMsg) -> Result<Binary, Error> {
    let res = match msg {
        QueryMsg::IcaSetSize {} => {
            let ica_set_size = state::ica_set_size(deps.storage);

            to_binary(&IcaSetSizeResponse { ica_set_size })?
        }

        QueryMsg::IcaMetadata { ica_idx } => {
            let ica_set_size = state::ica_set_size(deps.storage);

            if ica_idx >= ica_set_size {
                return Err(Error::IcaIndexOutOfBounds {
                    ica_idx,
                    ica_set_size,
                });
            }

            let maybe_ica_addr = state::ica_addr(deps.storage, ica_idx);

            let maybe_ica_icq_id = state::ica_icq_id(deps.storage, ica_idx);

            let metadata = maybe_ica_addr
                .zip(maybe_ica_icq_id)
                .map(|(address, icq_id)| IcaMetadata { address, icq_id });

            to_binary(&IcaMetadataResponse { metadata })?
        }

        QueryMsg::IcaLastBalance { ica_idx } => {
            let last_ica_balance = query_last_ica_balance(deps, env, ica_idx)?;
            to_binary(&last_ica_balance)?
        }
    };

    Ok(res)
}

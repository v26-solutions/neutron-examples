#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value
)]

pub mod msgs;

use common::{
    combine_u32s, debug, icq_deposit_fee, query_balance_icq, split_u64, OpenAckVersion,
    RemoteBalance,
};
use cosmwasm_std::{
    entry_point, from_slice, to_binary, Addr, BankMsg, Binary, Coin, CustomQuery, Deps, DepsMut,
    Env, MessageInfo, Reply, Response, SubMsg,
};
use neutron_sdk::{
    bindings::{
        msg::{IbcFee, NeutronMsg},
        query::NeutronQuery,
        types::ProtobufAny,
    },
    interchain_queries::v045::new_register_balance_query_msg,
    query::min_ibc_fee::query_min_ibc_fee,
    sudo::msg::{RequestPacket, RequestPacketTimeoutHeight, SudoMsg},
};
use prost::Message;
use serde::Serialize;

use crate::msgs::{
    ExecuteMsg, IcaLastBalance, IcaLastBalanceResponse, IcaMetadata, IcaMetadataResponse,
    IcaTxErrorResponse, IcaTxStatus, IcaTxStatusResponse, InstantiateMsg, QueryMsg,
};

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 60 * 60 * 24 * 7 * 2; // 2 weeks
pub const DEFAULT_TIMEOUT_HEIGHT: u64 = 10_000_000;

pub const REGISTER_ICQ_REPLY_KIND: u32 = 0;
pub const TRANSFER_TX_REPLY_KIND: u32 = 1;
pub const RETRIEVE_TX_REPLY_KIND: u32 = 2;

pub static IBC_FEE_DENOM: &str = "untrn";

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
    #[error("insufficient ibc tx fee")]
    InsufficientIbcTxFee,
    #[error("no ica setup")]
    NoIcaSetup,
    #[error("no funds to transfer")]
    NoFundsToTransfer,
    #[error("no funds to retrieve")]
    NoFundsToRetrieve,
    #[error("no funds expected")]
    NoFundsExpected,
    #[error("invalid rx hash")]
    InvalidRxHash,
}

macro_rules! hash {
    ($($part:expr),+) => {{
        let msg = [
            $( $part.as_ref(), )*
        ]
        .concat();

        let sha256 = hmac_sha256::Hash::hash(&msg);

        hex::encode_upper(sha256)
    }};
}

pub mod state {
    use common::{init_config, item, map};

    init_config!(connection_id        : String);
    init_config!(ibc_transfer_channel : String);
    init_config!(remote_denom         : String);
    init_config!(icq_update_period    : u64);
    init_config!(host_ibc_denom       : String);

    item!(ica_count : u32);

    map!(owner       : &str => ica_idx          : u32);
    map!(tx_hash     : &str => ica_idx          : u32);
    map!(rx_hash     : &str => ica_idx          : u32);
    map!(ica         : u32  => owner            : String);
    map!(ica         : u32  => addr             : String);
    map!(ica         : u32  => icq_id           : u64);
    map!(ica         : u32  => tx_issued_count  : u32);
    map!(ica         : u32  => tx_success_count : u32);
    map!(ica         : u32  => tx_error_count   : u32);
    map!(ica         : u32  => tx_timeout_count : u32);
    map!(ica         : u32  => round_trip_count : u32);
    map!(ica_tx_kind : u64  => seq_num          : u64);
    map!(ica_err_idx : u64  => msg              : String);
    map!(icq         : u64  => ica_idx          : u32);
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "handling instantiate msg");

    // save configuration
    state::set_connection_id(deps.storage, &msg.connection_id);

    state::set_ibc_transfer_channel(deps.storage, &msg.ibc_transfer_channel);

    state::set_icq_update_period(deps.storage, msg.icq_update_period);

    state::set_remote_denom(deps.storage, &msg.remote_denom);

    state::set_host_ibc_denom(deps.storage, &msg.host_ibc_denom);

    Ok(Response::default())
}

pub fn execute_setup_ica(
    deps: DepsMut<impl CustomQuery>,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "executing setup ica");

    let owner = info.sender.into_string();

    debug!(deps, "setting up ica for {owner}");

    // get required ICQ deposit fee
    let icq_deposit_fee = icq_deposit_fee(deps.as_ref())?;

    // check sender has provided the required funds for a single balance ICQ deposit
    let deposit = info.funds.first().ok_or(Error::IcqDepositMissing)?;

    if deposit.denom != icq_deposit_fee.denom {
        return Err(Error::IncorrectIcqDepositAsset);
    }

    if deposit.amount < icq_deposit_fee.amount {
        return Err(Error::InsufficientIcqDeposit);
    }

    let next_ica_idx = state::ica_count(deps.storage).unwrap_or_default();

    state::set_ica_count(deps.storage, next_ica_idx + 1);

    state::set_owner_ica_idx(deps.storage, &owner, next_ica_idx);

    state::set_ica_owner(deps.storage, next_ica_idx, &owner);

    let connection_id = state::connection_id(deps.storage);

    let registration_msg = NeutronMsg::RegisterInterchainAccount {
        connection_id,
        interchain_account_id: next_ica_idx.to_string(),
    };

    Ok(Response::default().add_message(registration_msg))
}

#[must_use]
pub fn is_ibc_fee_covered(info: &MessageInfo, ibc_fee: &IbcFee) -> bool {
    assert_eq!(ibc_fee.ack_fee.len(), 1, "only a single ibc ack fee asset");
    assert_eq!(
        ibc_fee.timeout_fee.len(),
        1,
        "only a single ibc timeout fee asset"
    );

    let Some(attached_fee_coin_amount) = info
        .funds
        .iter()
        .find_map(|c| (c.denom == IBC_FEE_DENOM).then_some(c.amount.u128()))
    else {
        return false;
    };

    let total_fee_amount: u128 = ibc_fee
        .timeout_fee
        .iter()
        .chain(ibc_fee.ack_fee.iter())
        .filter_map(|c| (c.denom == IBC_FEE_DENOM).then_some(c.amount.u128()))
        .sum();

    attached_fee_coin_amount >= total_fee_amount
}

pub fn execute_transfer_funds(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "executing transfer funds");

    let min_ibc_fee = query_min_ibc_fee(deps.as_ref()).map(|res| res.min_fee)?;

    if !is_ibc_fee_covered(&info, &min_ibc_fee) {
        return Err(Error::InsufficientIbcTxFee);
    }

    let tx_denom = state::host_ibc_denom(deps.storage);

    let tx_coin = info
        .funds
        .into_iter()
        .find(|c| c.denom == tx_denom)
        .ok_or(Error::NoFundsToTransfer)?;

    let owner = info.sender.as_str();

    let ica_idx = state::owner_ica_idx(deps.storage, owner).ok_or(Error::NoIcaSetup)?;

    let ica_addr = state::ica_addr(deps.storage, ica_idx).ok_or(Error::NoIcaSetup)?;

    let source_channel = state::ibc_transfer_channel(deps.storage);

    debug!(
        deps,
        "transfering {tx_coin} to {ica_addr} on behalf of {owner}"
    );

    let ibc_transfer_msg = NeutronMsg::IbcTransfer {
        source_port: "transfer".to_owned(),
        source_channel,
        sender: env.contract.address.into_string(),
        receiver: ica_addr,
        token: tx_coin,
        timeout_height: RequestPacketTimeoutHeight {
            revision_number: Some(2),
            revision_height: Some(DEFAULT_TIMEOUT_HEIGHT),
        },
        timeout_timestamp: 0,
        memo: String::new(),
        fee: min_ibc_fee,
    };

    let response = Response::default().add_submessage(SubMsg::reply_on_success(
        ibc_transfer_msg,
        combine_u32s(TRANSFER_TX_REPLY_KIND, ica_idx),
    ));

    Ok(response)
}

pub fn make_ibc_transfer_with_hook_msg<Msg: Serialize>(
    source_channel: String,
    token: Coin,
    sender: String,
    timeout_timestamp: u64,
    recipient: Addr,
    msg: Msg,
) -> ProtobufAny {
    #[derive(Clone, PartialEq, Message)]
    struct RawCoin {
        #[prost(string, tag = "1")]
        pub denom: String,
        #[prost(string, tag = "2")]
        pub amount: String,
    }

    impl From<Coin> for RawCoin {
        fn from(value: Coin) -> Self {
            Self {
                denom: value.denom,
                amount: value.amount.to_string(),
            }
        }
    }

    #[derive(Clone, PartialEq, Message)]
    struct Height {
        #[prost(uint64, tag = "1")]
        pub revision_number: u64,
        #[prost(uint64, tag = "2")]
        pub revision_height: u64,
    }

    #[derive(Clone, PartialEq, Message)]
    struct MsgTransfer {
        #[prost(string, tag = "1")]
        pub source_port: String,
        #[prost(string, tag = "2")]
        pub source_channel: String,
        #[prost(message, optional, tag = "3")]
        pub token: Option<RawCoin>,
        #[prost(string, tag = "4")]
        pub sender: String,
        #[prost(string, tag = "5")]
        pub receiver: String,
        #[prost(message, optional, tag = "6")]
        pub timeout_height: Option<Height>,
        #[prost(uint64, tag = "7")]
        pub timeout_timestamp: u64,
        #[prost(string, tag = "8")]
        pub memo: String,
    }

    #[derive(Serialize)]
    struct IbcHookWasm<Msg> {
        contract: String,
        msg: Msg,
    }

    #[derive(Serialize)]
    struct IbcHookMemo<Msg> {
        wasm: IbcHookWasm<Msg>,
    }

    let ibc_hook = IbcHookMemo {
        wasm: IbcHookWasm {
            contract: recipient.clone().into_string(),
            msg,
        },
    };

    let memo = serde_json_wasm::to_string(&ibc_hook).expect("infallible serialization");

    let transfer_msg = MsgTransfer {
        source_port: "transfer".to_owned(),
        source_channel,
        token: Some(token.into()),
        sender,
        receiver: recipient.into_string(),
        timeout_height: None,
        timeout_timestamp,
        memo,
    };

    ProtobufAny {
        type_url: "/ibc.applications.transfer.v1.MsgTransfer".to_owned(),
        value: transfer_msg.encode_to_vec().into(),
    }
}

pub fn execute_retrieve_funds(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "executing retrieve funds");

    let min_ibc_fee = query_min_ibc_fee(deps.as_ref()).map(|res| res.min_fee)?;

    if !is_ibc_fee_covered(&info, &min_ibc_fee) {
        return Err(Error::InsufficientIbcTxFee);
    }

    let owner = info.sender.as_str();

    let ica_idx = state::owner_ica_idx(deps.storage, owner).ok_or(Error::NoIcaSetup)?;

    let ica_balance_icq = state::ica_icq_id(deps.storage, ica_idx).ok_or(Error::NoIcaSetup)?;

    let non_zero_remote_balance = query_balance_icq(deps.as_ref(), ica_balance_icq)?
        .and_then(|res| res.balance)
        .filter(|remote_balance| !remote_balance.amount.is_zero())
        .ok_or(Error::NoFundsToRetrieve)?;

    let ica_addr = state::ica_addr(deps.storage, ica_idx).ok_or(Error::NoIcaSetup)?;

    let connection_id = state::connection_id(deps.storage);

    let source_channel = state::ibc_transfer_channel(deps.storage);

    let timeout_timestamp = env.block.time.plus_seconds(DEFAULT_TIMEOUT_SECONDS).nanos();

    let tx_idx = state::ica_tx_issued_count(deps.storage, ica_idx).unwrap_or_default();

    let rx_hash = hash!(
        ica_addr,
        non_zero_remote_balance.amount.u128().to_be_bytes(),
        tx_idx.to_be_bytes()
    );

    // save the ICA idx against the rx hash
    state::set_rx_hash_ica_idx(deps.storage, &rx_hash, ica_idx);

    let ibc_transfer_msg = make_ibc_transfer_with_hook_msg(
        source_channel,
        non_zero_remote_balance,
        ica_addr,
        timeout_timestamp,
        env.contract.address,
        // attach the rx hash to the callback message
        ExecuteMsg::FundsRetrievedHook { rx_hash },
    );

    let ica_submit_tx_msg = NeutronMsg::SubmitTx {
        connection_id,
        interchain_account_id: ica_idx.to_string(),
        msgs: vec![ibc_transfer_msg],
        memo: String::new(),
        timeout: DEFAULT_TIMEOUT_SECONDS,
        fee: min_ibc_fee,
    };

    let response = Response::default().add_submessage(SubMsg::reply_on_success(
        ica_submit_tx_msg,
        combine_u32s(RETRIEVE_TX_REPLY_KIND, ica_idx),
    ));

    Ok(response)
}

pub fn execute_funds_retrieved_hook(
    deps: DepsMut<impl CustomQuery>,
    info: MessageInfo,
    rx_hash: &str,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "executing retrieve funds: rx_hash = {rx_hash}");

    let ica_idx = state::rx_hash_ica_idx(deps.storage, rx_hash).ok_or(Error::InvalidRxHash)?;

    let current_round_trip_count =
        state::ica_round_trip_count(deps.storage, ica_idx).unwrap_or_default();

    state::set_ica_round_trip_count(deps.storage, ica_idx, current_round_trip_count + 1);

    let ica_owner = state::ica_owner(deps.storage, ica_idx).expect("ica must have an owner");

    // forward the funds recieved from the ICA to it's owner
    let msg = BankMsg::Send {
        to_address: ica_owner,
        amount: info.funds,
    };

    Ok(Response::default().add_message(msg))
}

#[entry_point]
pub fn execute(
    deps: DepsMut<NeutronQuery>,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "handling execute msg");

    match msg {
        ExecuteMsg::SetupIca {} => execute_setup_ica(deps, info),

        ExecuteMsg::TransferFunds {} => execute_transfer_funds(deps, env, info),

        ExecuteMsg::RetrieveFunds {} => execute_retrieve_funds(deps, env, info),

        ExecuteMsg::FundsRetrievedHook { rx_hash } => {
            execute_funds_retrieved_hook(deps, info, &rx_hash)
        }
    }
}

pub fn sudo_open_ack(
    deps: DepsMut<NeutronQuery>,
    port_id: String,
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

    let ica_idx = common::ica_idx_from_port_id(&port_id).expect("valid port id");

    let ica_addr = parsed_version.address;

    state::set_ica_addr(deps.storage, ica_idx, &ica_addr);

    let connection_id = state::connection_id(deps.storage);

    let icq_update_period = state::icq_update_period(deps.storage);

    let balance_icq_denom = state::remote_denom(deps.storage);

    let balance_icq_register_msg = new_register_balance_query_msg(
        connection_id.clone(),
        ica_addr.clone(),
        balance_icq_denom,
        icq_update_period,
    )?;

    let response = Response::default().add_submessage(SubMsg::reply_on_success(
        balance_icq_register_msg,
        combine_u32s(REGISTER_ICQ_REPLY_KIND, ica_idx),
    ));

    Ok(response)
}

pub fn sudo_response(
    deps: DepsMut<NeutronQuery>,
    request: RequestPacket,
) -> Result<Response<NeutronMsg>, Error> {
    let tx_seq_num = request.sequence.expect("sequence number always set");

    let source_channel = request.source_channel.expect("source channel always set");

    let tx_hash = hash!(tx_seq_num.to_be_bytes(), source_channel);

    debug!(deps, "received sudo response for tx: {tx_hash}");

    let ica_idx = state::tx_hash_ica_idx(deps.storage, &tx_hash)
        .expect("a sequence number is always associated with an ica idx");

    let mut tx_success_count =
        state::ica_tx_success_count(deps.storage, ica_idx).unwrap_or_default();

    tx_success_count += 1;

    debug!(
        deps,
        "ICA {ica_idx} issued tx with sequence number {tx_seq_num} successfully, total success count: {tx_success_count}"
    );

    state::set_ica_tx_success_count(deps.storage, ica_idx, tx_success_count);

    Ok(Response::default())
}

pub fn sudo_error(
    deps: DepsMut<NeutronQuery>,
    request: RequestPacket,
    error: String,
) -> Result<Response<NeutronMsg>, Error> {
    let tx_seq_num = request.sequence.expect("sequence number always set");

    let source_channel = request.source_channel.expect("source channel always set");

    let tx_hash = hash!(tx_seq_num.to_be_bytes(), source_channel);

    debug!(deps, "received sudo response for tx: {tx_hash}");

    let ica_idx = state::tx_hash_ica_idx(deps.storage, &tx_hash)
        .expect("a sequence number is always associated with an ica idx");

    let mut tx_error_count = state::ica_tx_error_count(deps.storage, ica_idx).unwrap_or_default();

    let error_key = combine_u32s(ica_idx, tx_error_count);

    tx_error_count += 1;

    debug!(
        deps,
        "ICA {ica_idx} issued tx with sequence number {tx_seq_num} failed: {error}, total error count: {tx_error_count}"
    );

    state::set_ica_tx_error_count(deps.storage, ica_idx, tx_error_count);

    state::set_ica_err_idx_msg(deps.storage, error_key, &error);

    Ok(Response::default())
}

pub fn sudo_timeout(
    deps: DepsMut<NeutronQuery>,
    request: RequestPacket,
) -> Result<Response<NeutronMsg>, Error> {
    let tx_seq_num = request.sequence.expect("sequence number always set");

    let source_channel = request.source_channel.expect("source channel always set");

    let tx_hash = hash!(tx_seq_num.to_be_bytes(), source_channel);

    debug!(deps, "received sudo response for tx: {tx_hash}");

    let ica_idx = state::tx_hash_ica_idx(deps.storage, &tx_hash)
        .expect("a sequence number is always associated with an ica idx");

    let mut tx_timeout_count =
        state::ica_tx_timeout_count(deps.storage, ica_idx).unwrap_or_default();

    tx_timeout_count += 1;

    debug!(
        deps,
        "ICA {ica_idx} issued tx with sequence number {tx_seq_num} timed out, total timeout count: {tx_timeout_count}"
    );

    state::set_ica_tx_timeout_count(deps.storage, ica_idx, tx_timeout_count);

    Ok(Response::default())
}

pub fn sudo_kv_query_result(
    deps: DepsMut<NeutronQuery>,
    query_id: u64,
) -> Result<Response<NeutronMsg>, Error> {
    let ica_idx =
        state::icq_ica_idx(deps.storage, query_id).expect("the icq is associated with an ica");

    let ica_addr = state::ica_addr(deps.storage, ica_idx).expect("the ica has an address");

    debug!(
        deps,
        "received balance ICQ {query_id} update for ICA {ica_idx} with address: {ica_addr}"
    );

    Ok(Response::default())
}

#[entry_point]
pub fn sudo(
    deps: DepsMut<NeutronQuery>,
    _env: Env,
    msg: SudoMsg,
) -> Result<Response<NeutronMsg>, Error> {
    debug!(deps, "handling sudo msg");

    match msg {
        SudoMsg::OpenAck {
            port_id,
            counterparty_version,
            ..
        } => sudo_open_ack(deps, port_id, counterparty_version),

        SudoMsg::Response { request, .. } => sudo_response(deps, request),

        SudoMsg::Error { request, details } => sudo_error(deps, request, details),

        SudoMsg::Timeout { request } => sudo_timeout(deps, request),

        SudoMsg::KVQueryResult { query_id } => sudo_kv_query_result(deps, query_id),

        SudoMsg::TxQueryResult { .. } => unimplemented!("not expecting tx query results"),
    }
}

pub fn reply_register_icq(deps: DepsMut, reply: Reply, ica_idx: u32) -> Result<Response, Error> {
    debug!(
        deps,
        "received icq registation reply for ICA index {ica_idx}",
    );

    let icq_id = common::parse_icq_registration_reply(reply)?;

    debug!(deps, "ICA {ica_idx} balance ICQ ID: {icq_id}",);

    state::set_ica_icq_id(deps.storage, ica_idx, icq_id);

    state::set_icq_ica_idx(deps.storage, icq_id, ica_idx);

    Ok(Response::default())
}

pub fn reply_issue_tx(
    deps: DepsMut,
    reply: Reply,
    tx_kind: u32,
    ica_idx: u32,
) -> Result<Response, Error> {
    debug!(deps, "received issue tx reply for ICA index {ica_idx}");

    let (tx_seq_num, channel) = common::parse_issue_tx_reply(reply)?;

    let tx_hash = hash!(tx_seq_num.to_be_bytes(), channel);

    state::set_tx_hash_ica_idx(deps.storage, &tx_hash, ica_idx);

    state::set_ica_tx_kind_seq_num(deps.storage, combine_u32s(ica_idx, tx_kind), tx_seq_num);

    let mut tx_issue_count = state::ica_tx_issued_count(deps.storage, ica_idx).unwrap_or_default();

    tx_issue_count += 1;

    debug!(
        deps,
        "ICA {ica_idx} issued tx {tx_issue_count} with sequence number {tx_seq_num}"
    );

    state::set_ica_tx_issued_count(deps.storage, ica_idx, tx_issue_count);

    Ok(Response::default())
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, reply: Reply) -> Result<Response, Error> {
    let (reply_kind, ica_idx) = split_u64(reply.id);

    debug!(
        deps,
        "received reply of kind {reply_kind} for ICA {ica_idx}"
    );

    match reply_kind {
        REGISTER_ICQ_REPLY_KIND => reply_register_icq(deps, reply, ica_idx),

        TRANSFER_TX_REPLY_KIND | RETRIEVE_TX_REPLY_KIND => {
            reply_issue_tx(deps, reply, reply_kind, ica_idx)
        }

        _ => unreachable!("unexpected reply kind: {reply_kind}"),
    }
}

pub fn owner_is_valid_addr(deps: Deps<impl CustomQuery>, owner: &str) -> Result<bool, Error> {
    deps.api.addr_validate(owner)?;

    Ok(true)
}

pub fn query_ica_metadata(
    deps: Deps<NeutronQuery>,
    owner: String,
) -> Result<IcaMetadataResponse, Error> {
    owner_is_valid_addr(deps, &owner)?;

    let Some(ica_idx) = state::owner_ica_idx(deps.storage, &owner) else {
        return Ok(IcaMetadataResponse::default());
    };

    let address = state::ica_addr(deps.storage, ica_idx);

    let balance_icq_id = state::ica_icq_id(deps.storage, ica_idx);

    Ok(IcaMetadataResponse {
        metadata: Some(IcaMetadata {
            ica_idx,
            address,
            balance_icq_id,
        }),
    })
}

pub fn query_last_ica_balance(
    deps: Deps<NeutronQuery>,
    owner: String,
) -> Result<IcaLastBalanceResponse, Error> {
    owner_is_valid_addr(deps, &owner)?;

    let Some(ica_idx) = state::owner_ica_idx(deps.storage, &owner) else {
        return Ok(IcaLastBalanceResponse::default());
    };

    let Some(icq_id) = state::ica_icq_id(deps.storage, ica_idx) else {
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

pub fn query_ica_tx_status(
    deps: Deps<impl CustomQuery>,
    owner: String,
) -> Result<IcaTxStatusResponse, Error> {
    owner_is_valid_addr(deps, &owner)?;

    let Some(ica_idx) = state::owner_ica_idx(deps.storage, &owner) else {
        return Ok(IcaTxStatusResponse::default());
    };

    let issued = state::ica_tx_issued_count(deps.storage, ica_idx).unwrap_or_default();

    let success = state::ica_tx_success_count(deps.storage, ica_idx).unwrap_or_default();

    let error = state::ica_tx_error_count(deps.storage, ica_idx).unwrap_or_default();

    let timeout = state::ica_tx_timeout_count(deps.storage, ica_idx).unwrap_or_default();

    let roundtrips = state::ica_round_trip_count(deps.storage, ica_idx).unwrap_or_default();

    let last_transfer_seq_num =
        state::ica_tx_kind_seq_num(deps.storage, combine_u32s(ica_idx, TRANSFER_TX_REPLY_KIND));

    let last_retrieve_seq_num =
        state::ica_tx_kind_seq_num(deps.storage, combine_u32s(ica_idx, RETRIEVE_TX_REPLY_KIND));

    let status = IcaTxStatus {
        issued,
        success,
        error,
        timeout,
        roundtrips,
        last_transfer_seq_num,
        last_retrieve_seq_num,
    };

    Ok(IcaTxStatusResponse {
        status: Some(status),
    })
}

pub fn query_ica_tx_error(
    deps: Deps<impl CustomQuery>,
    owner: String,
    error_idx: u32,
) -> Result<IcaTxErrorResponse, Error> {
    let Some(ica_idx) = state::owner_ica_idx(deps.storage, &owner) else {
        return Ok(IcaTxErrorResponse::default());
    };

    let error_key = combine_u32s(ica_idx, error_idx);

    let error = state::ica_err_idx_msg(deps.storage, error_key);

    Ok(IcaTxErrorResponse { error })
}

#[entry_point]
pub fn query(deps: Deps<NeutronQuery>, _env: Env, msg: QueryMsg) -> Result<Binary, Error> {
    let res = match msg {
        QueryMsg::IcaMetadata { owner } => {
            let ica_metadata = query_ica_metadata(deps, owner)?;

            to_binary(&ica_metadata)?
        }

        QueryMsg::IcaLastBalance { owner } => {
            let last_ica_balance = query_last_ica_balance(deps, owner)?;

            to_binary(&last_ica_balance)?
        }

        QueryMsg::IcaTxStatus { owner } => {
            let ica_tx_status = query_ica_tx_status(deps, owner)?;

            to_binary(&ica_tx_status)?
        }

        QueryMsg::IcaTxError { owner, error_idx } => {
            let ica_tx_status = query_ica_tx_error(deps, owner, error_idx)?;

            to_binary(&ica_tx_status)?
        }
    };

    Ok(res)
}

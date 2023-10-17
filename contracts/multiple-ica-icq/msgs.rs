use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Delegation};

#[cw_serde]
pub struct InstantiateMsg {
    /// The IBC connection ID on which to register ICAs/ICQs
    pub connection_id: String,
    /// The number of ICAs to register
    pub ica_set_size: u32,
    /// The target update period for ICQs
    pub icq_update_period: u64,
    /// The asset denomination of the balance ICQ
    pub balance_icq_denom: String,
    /// The validator of the delegations ICQ
    pub delegations_icq_validator: String,
}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
pub enum QueryMsg {
    IcaSetSize {},
    IcaMetadata { ica_idx: u32 },
    IcaLastBalance { ica_idx: u32 },
    IcaLastDelegation { ica_idx: u32 },
}

#[cw_serde]
pub struct IcaSetSizeResponse {
    pub ica_set_size: u32,
}

#[cw_serde]
pub struct IcaMetadata {
    pub address: String,
    pub balance_icq_id: u64,
    pub delegation_icq_id: u64,
}

#[cw_serde]
pub struct IcaMetadataResponse {
    pub metadata: Option<IcaMetadata>,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaLastBalance {
    pub balance: Option<Coin>,
    pub address: String,
    pub last_submitted_result_local_height: u64,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaLastBalanceResponse {
    pub last_balance: Option<IcaLastBalance>,
}

#[cw_serde]
pub struct IcaLastDelegation {
    pub delegation: Option<Delegation>,
    pub last_submitted_result_local_height: u64,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaLastDelegationResponse {
    pub last_delegation: Option<IcaLastDelegation>,
}

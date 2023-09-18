use cosmwasm_schema::cw_serde;
use cosmwasm_std::Coin;

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
}

#[cw_serde]
pub enum ExecuteMsg {}

#[cw_serde]
pub enum QueryMsg {
    IcaSetSize {},
    IcaMetadata { ica_idx: u32 },
    IcaLastBalance { ica_idx: u32 },
}

#[cw_serde]
pub struct IcaSetSizeResponse {
    pub ica_set_size: u32,
}

#[cw_serde]
pub struct IcaMetadata {
    pub address: String,
    pub icq_id: u64,
}

#[cw_serde]
pub struct IcaMetadataResponse {
    pub metadata: Option<IcaMetadata>,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaLastBalanceResponse {
    pub address: Option<String>,
    pub balance: Option<Coin>,
    pub last_local_update_height: Option<u64>,
}

#[cw_serde]
pub struct OpenAckVersion {
    pub version: String,
    pub controller_connection_id: String,
    pub host_connection_id: String,
    pub address: String,
    pub encoding: String,
    pub tx_type: String,
}

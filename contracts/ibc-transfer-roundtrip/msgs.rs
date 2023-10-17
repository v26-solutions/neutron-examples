use cosmwasm_schema::cw_serde;
use cosmwasm_std::Coin;

#[cw_serde]
pub struct InstantiateMsg {
    /// The IBC connection ID on which to register ICAs/ICQs
    pub connection_id: String,
    /// The IBC channel over which to transfer the assets
    pub ibc_transfer_channel: String,
    /// The target update period for ICQs
    pub icq_update_period: u64,
    /// The denom of the transfer asset on the remote chain
    pub remote_denom: String,
    /// The ICS-20 denom of the transfer asset on the host chain
    pub host_ibc_denom: String,
}

#[cw_serde]
pub enum ExecuteMsg {
    /// Setup an ICA for the sender to transfer assets to
    SetupIca {},
    /// Transfer attached funds to the ICA if one has been setup
    TransferFunds {},
    /// Retrieve funds from the ICA if one has been setup and it has a non-zero balance
    RetrieveFunds {},
    /// Callback for when funds are retrieved from the ICA
    FundsRetrievedHook {
        /// IBC hook sender cannot be trusted - this has is used to identify the sender ICA
        rx_hash: String,
    },
}

#[cw_serde]
pub enum QueryMsg {
    /// Query the metadata for the ICA setup by the `owner` address, if any
    IcaMetadata { owner: String },
    /// Query the last transfer asset balance for the ICA setup by the `owner` address, if any
    IcaLastBalance { owner: String },
    /// Query the ICA Tx status data for the `owner` address, if any
    IcaTxStatus { owner: String },
    /// Query the error message for the `error_idx` and `owner` address, if any
    IcaTxError { owner: String, error_idx: u32 },
}

#[cw_serde]
pub struct IcaMetadata {
    pub ica_idx: u32,
    pub address: Option<String>,
    pub balance_icq_id: Option<u64>,
}

#[cw_serde]
#[derive(Default)]
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
pub struct IcaTxStatus {
    pub issued: u32,
    pub success: u32,
    pub error: u32,
    pub timeout: u32,
    pub roundtrips: u32,
    pub last_transfer_seq_num: Option<u64>,
    pub last_retrieve_seq_num: Option<u64>,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaTxStatusResponse {
    pub status: Option<IcaTxStatus>,
}

#[cw_serde]
#[derive(Default)]
pub struct IcaTxErrorResponse {
    pub error: Option<String>,
}

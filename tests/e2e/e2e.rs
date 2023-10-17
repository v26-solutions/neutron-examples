use std::{sync::OnceLock, time::SystemTime};

use ::multiple_ica_icq::msgs::{IcaLastBalance, IcaLastDelegation, IcaLastDelegationResponse};
use anyhow::Result;
use serde::Serialize;
use xshell::Shell;

use cosmwasm_std::Coin;
use cosmwasm_xtask::{
    execute, instantiate,
    key::Key,
    network::{gas::Price as GasPrice, neutron::local::GAIA_CHAIN_ID, Instance},
    query, store, wait_for_blocks, Initialize, Network, NeutronLocalnet,
};

pub struct Ctx {
    pub sh: Shell,
    pub network: Instance<NeutronLocalnet>,
}

pub fn pretty<T: Serialize>(t: &T) -> String {
    ron::ser::to_string_pretty(
        t,
        ron::ser::PrettyConfig::default()
            .indentor("  ".to_owned())
            .separate_tuple_members(false),
    )
    .unwrap()
}

pub fn setup() -> Result<Ctx> {
    type DistResult = std::result::Result<(), cosmwasm_xtask::Error>;

    static DIST: OnceLock<DistResult> = OnceLock::new();

    let sh = Shell::new()?;

    // change shell directory to workspace route
    sh.change_dir("../../");

    let network = NeutronLocalnet::initialize(&sh)?;

    if wait_for_blocks(&sh, &network).is_err() {
        panic!(
            r#"
                Local network is not running.
                Either start a local network in another terminal with the command `cargo x start-local`,
                or run tests using the command `cargo x test e2e <optionally-specify-test-case>`.
            "#
        )
    }

    if std::env::var("E2E_NO_DIST").is_err() {
        if let Err(err) = DIST.get_or_init(|| {
            // build contracts for distribution
            cosmwasm_xtask::ops::dist_workspace(&sh)
        }) {
            anyhow::bail!("{err}");
        }
    }

    Ok(Ctx { sh, network })
}

macro_rules! test_contract {
    ($f:ident) => {
        mod $f {
            #[test]
            fn works() -> anyhow::Result<()> {
                let ctx = super::setup()?;

                let key = ctx.network.keys.first().unwrap();

                super::$f(&ctx.sh, &ctx.network, key)?;

                Ok(())
            }
        }
    };

    (test_case: $f:ident, prerequisites: [$($prereq:ident),+]) => {
        mod $f {
            #[test]
            fn works() -> anyhow::Result<()> {
                let ctx = super::setup()?;

                let key = ctx.network.keys.first().unwrap();

                $(
                    super::$prereq(&ctx, key)?;
                )+

                super::$f(&ctx.sh, &ctx.network, key)?;

                Ok(())
            }
        }
    };
}

pub fn label(prefix: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    format!("{prefix}:{timestamp}")
}

pub fn multiple_ica_icq(sh: &Shell, network: &dyn Network, key: &Key) -> Result<()> {
    use ::multiple_ica_icq::msgs::{
        IcaLastBalanceResponse, IcaMetadataResponse, InstantiateMsg, QueryMsg,
    };

    let contract_path = "artifacts/multiple_ica_icq.wasm";

    let ica_set_size = 10;

    eprintln!("storing contract: {contract_path}");

    let code_id = store(contract_path).send(sh, network, key)?;

    let init_msg = InstantiateMsg {
        connection_id: "connection-0".to_owned(),
        ica_set_size,
        icq_update_period: 6,
        balance_icq_denom: "uatom".to_owned(),
        delegations_icq_validator: "cosmosvaloper18hl5c9xn5dze2g50uaw0l2mr02ew57zk0auktn"
            .to_owned(),
    };

    let deposit = 1_000_000 * u128::from(ica_set_size) * 2;

    eprintln!(
        "instantiating contract code {code_id} with {deposit}untrn & params: {}",
        pretty(&init_msg)
    );

    let contract = instantiate(code_id, &label("multiple_ica_icq"), init_msg)
        // 2 ICQ deposits per ICA
        .amount(deposit, "untrn")
        .send(sh, network, key)?;

    eprintln!("waiting for ICAs and ICQs to be registered...");

    let mut ica_idx = 0;

    let mut block_count = 0;

    loop {
        let ica_metadata_res: IcaMetadataResponse =
            query(sh, network, &contract, &QueryMsg::IcaMetadata { ica_idx })?;

        if let Some(metadata) = ica_metadata_res.metadata {
            eprintln!(
                "multiple_ica_icq: ICA {ica_idx} registered: {}",
                pretty(&metadata)
            );

            ica_idx += 1;

            if ica_idx == ica_set_size {
                break;
            }

            continue;
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    eprintln!("all {ica_set_size} ICAs with 2 ICQs each registered in {block_count} blocks");

    eprintln!("waiting for first balance ICQ results to be posted...");

    let mut ica_idx = 0;

    let mut block_count = 0;

    loop {
        if let IcaLastBalanceResponse {
            last_balance:
                Some(IcaLastBalance {
                    balance,
                    address,
                    last_submitted_result_local_height,
                }),
        } = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaLastBalance { ica_idx },
        )? {
            let balance_msg = balance
                .as_ref()
                .map_or_else(|| "empty balance".to_owned(), Coin::to_string);

            eprintln!("multiple_ica_icq: ICA {ica_idx} {address} last balance: {balance_msg} updated at height {last_submitted_result_local_height}");

            ica_idx += 1;

            if ica_idx == 10 {
                break;
            }

            continue;
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    eprintln!("all {ica_set_size} balance ICQs have results after {block_count} blocks");

    eprintln!("waiting for first delegation ICQ results to be posted");

    let mut ica_idx = 0;

    let mut block_count = 0;

    loop {
        if let IcaLastDelegationResponse {
            last_delegation:
                Some(IcaLastDelegation {
                    delegation,
                    last_submitted_result_local_height,
                }),
        } = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaLastDelegation { ica_idx },
        )? {
            let delegation_msg = delegation
                .as_ref()
                .map_or_else(|| "not yet delegated".to_owned(), pretty);

            eprintln!("multiple_ica_icq: ICA {ica_idx} last delegation: {delegation_msg} updated at height {last_submitted_result_local_height}");

            ica_idx += 1;

            if ica_idx == 10 {
                break;
            }

            continue;
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    eprintln!("all {ica_set_size} delegation ICQs have results after {block_count} blocks");

    Ok(())
}

test_contract!(multiple_ica_icq);

pub fn ibc_transfer_atom_to_neutron(Ctx { sh, network }: &Ctx, key: &Key) -> Result<()> {
    let chain_id = GAIA_CHAIN_ID.to_owned().into();

    let node_uri = network.gaiad.node_uri();

    let gas = GasPrice::new(0.02, "uatom").units(200_000);

    network
        .gaiad
        .cli(sh)
        .tx(key, &chain_id, &node_uri)
        .ibc_transfer("channel-0", key.address(), 10_000_000_000, "uatom")
        .execute(&gas)?;

    Ok(())
}

pub fn ibc_transfer_roundtrip(sh: &Shell, network: &dyn Network, key: &Key) -> Result<()> {
    use ::ibc_transfer_roundtrip::msgs::{
        ExecuteMsg, IcaLastBalance, IcaLastBalanceResponse, IcaMetadataResponse,
        IcaTxStatusResponse, InstantiateMsg, QueryMsg,
    };

    let contract_path = "artifacts/ibc_transfer_roundtrip.wasm";

    eprintln!("storing contract: {contract_path}");

    let code_id = store(contract_path).send(sh, network, key)?;

    let ibc_atom_denom = "ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2";

    let init_msg = InstantiateMsg {
        connection_id: "connection-0".to_owned(),
        ibc_transfer_channel: "channel-0".to_owned(),
        icq_update_period: 6,
        remote_denom: "uatom".to_owned(),
        host_ibc_denom: ibc_atom_denom.to_owned(),
    };

    eprintln!(
        "instantiating contract code {code_id} with params: {}",
        pretty(&init_msg)
    );

    let contract =
        instantiate(code_id, &label("ibc_transfer_roundtrip"), init_msg).send(sh, network, key)?;

    eprintln!("instantiated contract with address: {contract}");

    eprintln!("setting up an ICA for {key}");

    execute(&contract, ExecuteMsg::SetupIca {})
        .amount(1_000_000, "untrn")
        .send(sh, network, key)?;

    let mut block_count = 0;

    loop {
        let ica_metadata_res: IcaMetadataResponse = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaMetadata {
                owner: key.address().to_owned(),
            },
        )?;

        if let Some(metadata) = ica_metadata_res.metadata {
            if let Some((address, balance_icq)) = metadata.address.zip(metadata.balance_icq_id) {
                eprintln!(
                    "ICA {} registeration with address {address} and balance ICQ {balance_icq} took {block_count} blocks",
                    metadata.ica_idx
                );
                break;
            }
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    let node_uri = network.node_uri(sh)?;

    eprintln!("waiting for IBC ATOM to be sent");

    let original_ibc_atom_balance = loop {
        let balance = network
            .cli(sh)?
            .query(&node_uri)
            .balance(key.address(), ibc_atom_denom)?;

        if balance >= 1_000_000_000 {
            break balance;
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    };

    eprintln!("{key} starting off with {original_ibc_atom_balance} IBC ATOM");

    eprintln!("transferring IBC ATOM to ICA");

    execute(&contract, ExecuteMsg::TransferFunds {})
        .amount(2000, "untrn")
        .amount(1_000_000_000, ibc_atom_denom)
        .send(sh, network, key)?;

    let mut block_count = 0;

    loop {
        if let IcaLastBalanceResponse {
            last_balance:
                Some(IcaLastBalance {
                    balance: Some(balance),
                    address,
                    last_submitted_result_local_height,
                }),
        } = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaLastBalance {
                owner: key.address().to_owned(),
            },
        )? {
            eprintln!(
                "ICA with address {} has {} at local height {} after waiting {} blocks",
                address, balance, last_submitted_result_local_height, block_count
            );
            break;
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    let current_ibc_atom_balance = network
        .cli(sh)?
        .query(&node_uri)
        .balance(key.address(), ibc_atom_denom)?;

    assert_eq!(
        current_ibc_atom_balance,
        original_ibc_atom_balance - 1_000_000_000
    );

    eprintln!("retrieving ATOM from ICA");

    execute(&contract, ExecuteMsg::RetrieveFunds {})
        .amount(2000, "untrn")
        .send(sh, network, key)?;

    let mut block_count = 0;

    loop {
        if let IcaTxStatusResponse {
            status: Some(status),
        } = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaTxStatus {
                owner: key.address().to_owned(),
            },
        )? {
            if status.roundtrips > 0 {
                break;
            }
        }

        eprintln!("waiting for another block...");

        wait_for_blocks(sh, network)?;

        block_count += 1;
    }

    eprintln!("funds retrieved after {block_count} blocks");

    let current_ibc_atom_balance = network
        .cli(sh)?
        .query(&node_uri)
        .balance(key.address(), ibc_atom_denom)?;

    assert_eq!(current_ibc_atom_balance, original_ibc_atom_balance);

    Ok(())
}

test_contract! {
    test_case: ibc_transfer_roundtrip,
    prerequisites: [
        ibc_transfer_atom_to_neutron
    ]
}

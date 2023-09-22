use std::{sync::OnceLock, time::SystemTime};

use ::multiple_ica_icq::msgs::{IcaLastBalance, IcaLastDelegation, IcaLastDelegationResponse};
use anyhow::Result;
use serde::Serialize;
use xshell::Shell;

use cosmwasm_std::Coin;
use cosmwasm_xtask::{
    instantiate, key::Key, network::Instance, query, store, wait_for_blocks, Initialize, Network,
    NeutronLocalnet,
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

use std::{sync::OnceLock, time::SystemTime};

use anyhow::Result;
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

pub fn setup() -> Result<Ctx> {
    static DIST: OnceLock<std::result::Result<(), cosmwasm_xtask::Error>> = OnceLock::new();

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
        IcaLastBalanceResponse, IcaMetadata, IcaMetadataResponse, InstantiateMsg, QueryMsg,
    };

    let code_id = store("artifacts/multiple_ica_icq.wasm").send(sh, network, key)?;

    let contract = instantiate(
        code_id,
        &label("multiple_ica_icq"),
        InstantiateMsg {
            connection_id: "connection-0".to_owned(),
            ica_set_size: 10,
            icq_update_period: 6,
            balance_icq_denom: "uatom".to_owned(),
        },
    )
    .amount(1_000_000 * 10, "untrn")
    .send(sh, network, key)?;

    println!("wait for ICAs and ICQs to be registered");

    let mut ica_idx = 0;

    loop {
        let ica_metadata_res: IcaMetadataResponse =
            query(sh, network, &contract, &QueryMsg::IcaMetadata { ica_idx })?;

        if let Some(IcaMetadata { address, icq_id }) = ica_metadata_res.metadata {
            println!("multiple_ica_icq: ICA {ica_idx} registered: address = {address}, icq_id = {icq_id}");

            ica_idx += 1;

            if ica_idx == 10 {
                break;
            }

            continue;
        }

        // wait until the next block
        wait_for_blocks(sh, network)?;
    }

    println!("wait for ICQ results to be posted");

    let mut ica_idx = 0;

    loop {
        let IcaLastBalanceResponse {
            address,
            balance,
            last_local_update_height,
        } = query(
            sh,
            network,
            &contract,
            &QueryMsg::IcaLastBalance { ica_idx },
        )?;

        let address = address.expect("already waited for ICA registration");

        let balance_msg = balance
            .as_ref()
            .map_or_else(|| "empty balance".to_owned(), Coin::to_string);

        if let Some(last_local_update_height) = last_local_update_height {
            println!("multiple_ica_icq: ICA {ica_idx} {address} last balance: {balance_msg} updated at height {last_local_update_height}");
            ica_idx += 1;

            if ica_idx == 10 {
                break;
            }

            continue;
        }

        // wait until the next block
        wait_for_blocks(sh, network)?;
    }

    Ok(())
}

test_contract!(multiple_ica_icq);

use common::updated_registered_kv_query;
use cosmwasm_std::Deps;
use neutron_sdk::{
    bindings::query::NeutronQuery,
    interchain_queries::{query_kv_result, v045::types::Delegations},
    NeutronError,
};

use crate::msgs::IcaLastDelegation;

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

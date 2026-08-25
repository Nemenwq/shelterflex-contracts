//! Authorization-boundary tests: every admin-gated entry point must reject a
//! non-admin caller.

extern crate std;

use super::*;
use soroban_sdk::{testutils::Address as _, Env};

fn setup(env: &Env) -> (Address, Address, GovernanceClient<'_>) {
    env.mock_all_auths();
    let id = env.register(Governance, ());
    let client = GovernanceClient::new(env, &id);
    let admin = Address::generate(env);
    let attacker = Address::generate(env);
    client.init(&admin, &1_000_000);
    (admin, attacker, client)
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let env = Env::default();
    let (_admin, attacker, client) = setup(&env);
    let other = Address::generate(&env);

    assert_eq!(
        client
            .try_set_total_staked(&attacker, &2_000_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_total_staked must reject a non-admin"
    );

    assert_eq!(
        client
            .try_set_staking_pool(&attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_staking_pool must reject a non-admin"
    );

    assert_eq!(
        client
            .try_set_voter_stake(&attacker, &other, &100_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_voter_stake must reject a non-admin"
    );
}

/// A non-admin must not be able to hand itself voting weight.
#[test]
fn rejected_stake_grant_does_not_change_state() {
    let env = Env::default();
    let (_admin, attacker, client) = setup(&env);

    let result = client.try_set_voter_stake(&attacker, &attacker, &500_000);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    // With no stake, the attacker still cannot clear the proposal threshold.
    let result = client.try_create_proposal(&attacker, &Symbol::new(&env, "fee_bps"), &100, &200);
    assert!(
        result.is_err(),
        "a rejected stake grant must leave the attacker without voting weight"
    );
}

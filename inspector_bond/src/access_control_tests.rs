//! Authorization-boundary tests: every admin-gated entry point must reject a
//! non-admin caller.

extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::Env;

fn setup(env: &Env) -> (Address, Address, InspectorBondContractClient<'_>) {
    env.mock_all_auths();
    let id = env.register(InspectorBondContract, ());
    let client = InspectorBondContractClient::new(env, &id);
    let admin = Address::generate(env);
    let attacker = Address::generate(env);
    client.init(&admin, &1_000, &1_000, &0);
    (admin, attacker, client)
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let env = Env::default();
    let (admin, attacker, client) = setup(&env);
    let inspector = Address::generate(&env);
    let report_id = BytesN::from_array(&env, &[1u8; 32]);

    client.stake_bond(&inspector, &10_000);
    let slash_id = client.propose_inspector_slash(
        &admin,
        &inspector,
        &report_id,
        &String::from_str(&env, "reason"),
        &SlashSeverity::Low,
    );

    assert_eq!(
        client
            .try_set_min_bond(&attacker, &2_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_min_bond must reject a non-admin"
    );

    assert_eq!(
        client
            .try_set_inspector_challenge_window(&attacker, &600)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_inspector_challenge_window must reject a non-admin"
    );

    assert_eq!(
        client
            .try_slash_inspector(
                &attacker,
                &inspector,
                &report_id,
                &Symbol::new(&env, "reason"),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "slash_inspector must reject a non-admin"
    );

    assert_eq!(
        client
            .try_propose_inspector_slash(
                &attacker,
                &inspector,
                &report_id,
                &String::from_str(&env, "reason"),
                &SlashSeverity::Low,
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "propose_inspector_slash must reject a non-admin"
    );

    assert_eq!(
        client
            .try_finalize_inspector_slash(&attacker, &slash_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "finalize_inspector_slash must reject a non-admin"
    );

    assert_eq!(
        client
            .try_cancel_inspector_slash(&attacker, &slash_id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "cancel_inspector_slash must reject a non-admin"
    );

    assert_eq!(
        client.try_pause(&attacker).unwrap_err().unwrap(),
        ContractError::NotAuthorized,
        "pause must reject a non-admin"
    );

    client.pause(&admin);
    assert_eq!(
        client.try_unpause(&attacker).unwrap_err().unwrap(),
        ContractError::NotAuthorized,
        "unpause must reject a non-admin"
    );
}

/// A rejected slash must leave the inspector's bond untouched.
#[test]
fn rejected_slash_leaves_bond_intact() {
    let env = Env::default();
    let (_admin, attacker, client) = setup(&env);
    let inspector = Address::generate(&env);
    let report_id = BytesN::from_array(&env, &[2u8; 32]);

    client.stake_bond(&inspector, &10_000);
    let before = client.get_bond(&inspector).unwrap().amount;

    let result =
        client.try_slash_inspector(&attacker, &inspector, &report_id, &Symbol::new(&env, "r"));
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(client.get_bond(&inspector).unwrap().amount, before);
}

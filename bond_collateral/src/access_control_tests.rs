//! Authorization-boundary tests: every gated entry point must reject a caller
//! that holds neither the admin nor the operator role, and every state mutation
//! must be refused while paused.

extern crate std;

use super::*;
use slashing_module::{SlashingModule, SlashingModuleClient};
use soroban_pausable_core::PausableError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

struct Setup<'a> {
    env: Env,
    bond: BondCollateralClient<'a>,
    admin: Address,
    operator: Address,
    inspector: Address,
    attacker: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let inspector = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token = Address::generate(&env);

    let bond_id = env.register(BondCollateral, ());
    let bond = BondCollateralClient::new(&env, &bond_id);
    bond.init(&admin, &token);

    let slasher_id = env.register(SlashingModule, ());
    let slasher = SlashingModuleClient::new(&env, &slasher_id);
    slasher.init(&admin);
    slasher.set_bond_contract(&admin, &bond_id);

    bond.set_slashing_module(&admin, &slasher_id);
    bond.set_operator(&admin, &operator);
    bond.deposit_bond(&inspector, &10_000);

    Setup {
        env,
        bond,
        admin,
        operator,
        inspector,
        attacker,
    }
}

fn inspection(env: &Env, s: &str) -> String {
    String::from_str(env, s)
}

// ── Non-admin is rejected on every admin-gated entry point ───────────────────

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let s = setup();
    let other = Address::generate(&s.env);

    assert_eq!(
        s.bond
            .try_set_admin(&s.attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_admin must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_thresholds(&s.attacker, &150, &120)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_thresholds must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_keeper_reward_cap(&s.attacker, &100)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_keeper_reward_cap must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_oracle_feed(&s.attacker, &other, &600)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_oracle_feed must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_target_health_ratio(&s.attacker, &150)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_target_health_ratio must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_slashing_module(&s.attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_slashing_module must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_set_operator(&s.attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_operator must reject a non-admin"
    );

    assert_eq!(
        s.bond
            .try_execute_slash(
                &s.attacker,
                &s.inspector,
                &100,
                &inspection(&s.env, "INSP-1"),
                &inspection(&s.env, "reason"),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "execute_slash must reject a non-admin"
    );

    assert_eq!(
        s.bond.try_pause(&s.attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "pause must reject a non-admin"
    );

    s.bond.pause(&s.admin);
    assert_eq!(
        s.bond.try_unpause(&s.attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "unpause must reject a non-admin"
    );
}

/// The operator role is narrower than admin: it must not open the admin gate,
/// and a non-operator must not open the operator gate.
#[test]
fn operator_gate_is_distinct_from_admin_gate() {
    let s = setup();

    assert_eq!(
        s.bond
            .try_set_operator(&s.operator, &s.attacker)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "operator must not pass the admin gate"
    );

    assert_eq!(
        s.bond
            .try_lock_bond(&s.attacker, &s.inspector, &inspection(&s.env, "INSP-1"))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "lock_bond must reject a non-operator"
    );

    assert_eq!(
        s.bond
            .try_unlock_bond(&s.attacker, &s.inspector, &inspection(&s.env, "INSP-1"))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "unlock_bond must reject a non-operator"
    );

    assert_eq!(
        s.bond
            .try_lock_bond(&s.admin, &s.inspector, &inspection(&s.env, "INSP-1"))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "admin must not pass the operator gate"
    );
}

/// A rejected call must leave the configuration and the bond ledger untouched.
#[test]
fn rejected_call_does_not_change_state() {
    let s = setup();

    let thresholds_before = s.bond.get_thresholds();
    let bond_before = s.bond.get_bond(&s.inspector);

    let result = s.bond.try_set_thresholds(&s.attacker, &160, &130);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s.bond.try_execute_slash(
        &s.attacker,
        &s.inspector,
        &100,
        &inspection(&s.env, "INSP-1"),
        &inspection(&s.env, "reason"),
    );
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(s.bond.get_thresholds(), thresholds_before);
    assert_eq!(s.bond.get_bond(&s.inspector), bond_before);
}

// ── Pause blocks state mutation and lifts on unpause ─────────────────────────

#[test]
fn contract_starts_unpaused() {
    let s = setup();
    assert!(!s.bond.is_paused());
}

#[test]
fn admin_can_pause_and_unpause() {
    let s = setup();

    s.bond.pause(&s.admin);
    assert!(s.bond.is_paused());

    s.bond.unpause(&s.admin);
    assert!(!s.bond.is_paused());
}

#[test]
fn paused_contract_rejects_admin_configuration_changes() {
    let s = setup();
    let other = Address::generate(&s.env);
    s.bond.pause(&s.admin);

    assert_eq!(
        s.bond.try_set_admin(&s.admin, &other).unwrap_err().unwrap(),
        ContractError::Paused,
        "set_admin must be refused while paused"
    );

    assert_eq!(
        s.bond
            .try_set_thresholds(&s.admin, &150, &120)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "set_thresholds must be refused while paused"
    );

    assert_eq!(
        s.bond
            .try_set_operator(&s.admin, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "set_operator must be refused while paused"
    );

    assert_eq!(
        s.bond
            .try_execute_slash(
                &s.admin,
                &s.inspector,
                &100,
                &inspection(&s.env, "INSP-1"),
                &inspection(&s.env, "reason"),
            )
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "execute_slash must be refused while paused"
    );
}

#[test]
fn state_mutation_resumes_after_unpause() {
    let s = setup();

    s.bond.pause(&s.admin);
    assert_eq!(
        s.bond
            .try_deposit_bond(&s.inspector, &500)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );

    s.bond.unpause(&s.admin);
    s.bond.deposit_bond(&s.inspector, &500);
    assert_eq!(s.bond.get_bond(&s.inspector), 10_500);
}

/// Pausing must not touch the bond ledger: balances on either side of a
/// pause/unpause cycle are identical.
#[test]
fn pause_cycle_preserves_bond_state() {
    let s = setup();

    let before = s.bond.get_bond(&s.inspector);
    let total_before = s.bond.total_collateral();

    s.bond.pause(&s.admin);
    s.bond.unpause(&s.admin);

    assert_eq!(s.bond.get_bond(&s.inspector), before);
    assert_eq!(s.bond.total_collateral(), total_before);
}

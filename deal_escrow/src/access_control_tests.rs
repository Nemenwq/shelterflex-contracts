//! Authorization-boundary tests: every gated entry point must reject a caller
//! that holds neither the admin nor the operator role.

extern crate std;

use crate::{ContractError, DealEscrow, DealEscrowClient};
use soroban_pausable_core::PausableError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, String, Symbol, Vec};

struct Setup<'a> {
    env: Env,
    client: DealEscrowClient<'a>,
    admin: Address,
    operator: Address,
    attacker: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(DealEscrow, ());
    let client = DealEscrowClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let receipt_contract = Address::generate(&env);

    client.init(&admin, &operator, &token, &receipt_contract);

    Setup {
        env,
        client,
        admin,
        operator,
        attacker,
    }
}

fn deal_id(env: &Env) -> String {
    String::from_str(env, "deal-1")
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let s = setup();
    let other = Address::generate(&s.env);
    let hash = BytesN::from_array(&s.env, &[3u8; 32]);
    let id = deal_id(&s.env);

    assert_eq!(
        s.client
            .try_configure_dispute_windows(&s.attacker, &600, &1_200)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "configure_dispute_windows must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_resolver(&s.attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_resolver must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_migrate_storage_schema(&s.attacker, &1, &Vec::new(&s.env))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "migrate_storage_schema must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_guardian(&s.attacker, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_guardian must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_upgrade_delay(&s.attacker, &100)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_upgrade_delay must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_propose_upgrade(&s.attacker, &hash)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "propose_upgrade must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_execute_upgrade(&s.attacker, &hash)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "execute_upgrade must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_emergency_upgrade(&s.attacker, &hash)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "emergency_upgrade must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_cancel_upgrade(&s.attacker)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "cancel_upgrade must reject a non-admin"
    );

    assert_eq!(
        s.client.try_freeze(&s.attacker).unwrap_err().unwrap(),
        ContractError::NotAuthorized,
        "freeze must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_propose_drain(&s.attacker, &hash)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "propose_drain must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_execute_drain(&s.attacker, &hash)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "execute_drain must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_recovery_delay(&s.attacker, &100)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_recovery_delay must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_activate_deal(&s.attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "activate_deal must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_complete_deal(&s.attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "complete_deal must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_default_deal(&s.attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "default_deal must reject a non-admin"
    );

    assert_eq!(
        s.client.try_pause(&s.attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "pause must reject a non-admin"
    );

    s.client.pause(&s.admin);
    assert_eq!(
        s.client.try_unpause(&s.attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "unpause must reject a non-admin"
    );
}

/// The operator role is narrower than admin: it must not open the admin gate.
#[test]
fn operator_does_not_pass_the_admin_gate() {
    let s = setup();
    let other = Address::generate(&s.env);

    assert_eq!(
        s.client
            .try_set_resolver(&s.operator, &other)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "operator must not pass the admin gate"
    );

    assert_eq!(
        s.client.try_freeze(&s.operator).unwrap_err().unwrap(),
        ContractError::NotAuthorized,
        "operator must not be able to freeze the escrow"
    );
}

/// `release` accepts admin *or* operator, and nobody else — a non-role caller
/// must not be able to move escrowed funds.
#[test]
fn release_rejects_a_caller_with_no_role() {
    let s = setup();
    let id = deal_id(&s.env);
    let external_ref = String::from_str(&s.env, "ref-1");
    let recipient = Address::generate(&s.env);

    let result = s.client.try_release(
        &s.attacker,
        &id,
        &recipient,
        &100,
        &recipient,
        &10,
        &recipient,
        &10,
        &Symbol::new(&s.env, "manual"),
        &external_ref,
    );
    assert_eq!(
        result.unwrap_err().unwrap(),
        ContractError::NotAuthorized,
        "release must reject a caller that is neither admin nor operator"
    );
}

/// A rejected call must leave the escrow untouched.
#[test]
fn rejected_call_does_not_change_state() {
    let s = setup();
    let id = deal_id(&s.env);
    let hash = BytesN::from_array(&s.env, &[4u8; 32]);

    let balance_before = s.client.balance(&id);
    let frozen_before = s.client.is_frozen();

    let result = s.client.try_propose_drain(&s.attacker, &hash);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s.client.try_freeze(&s.attacker);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(s.client.balance(&id), balance_before);
    assert_eq!(s.client.is_frozen(), frozen_before);
}

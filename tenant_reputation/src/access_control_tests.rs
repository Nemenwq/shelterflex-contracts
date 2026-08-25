//! Authorization-boundary tests: every gated entry point must reject a caller
//! that holds neither the admin nor the operator role.

extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Symbol};

struct Setup<'a> {
    env: Env,
    client: TenantReputationClient<'a>,
    admin: Address,
    operator: Address,
    attacker: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(TenantReputation, ());
    let client = TenantReputationClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.init(&admin, &operator);

    Setup {
        env,
        client,
        admin,
        operator,
        attacker,
    }
}

fn sample_record(env: &Env) -> ReputationRecord {
    ReputationRecord {
        composite_score: 750,
        payment_score: 80,
        property_care_score: 70,
        communication_score: 90,
        total_ratings: 5,
        last_updated: env.ledger().timestamp(),
    }
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let s = setup();
    let tenant = Address::generate(&s.env);

    assert_eq!(
        s.client
            .try_set_decay_config(&s.attacker, &10, &86_400)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_decay_config must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_score_bounds(&s.attacker, &0, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_score_bounds must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_revoke_reputation(&s.attacker, &tenant)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "revoke_reputation must reject a non-admin"
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

/// `update_reputation` accepts admin *or* operator, and nobody else.
#[test]
fn update_reputation_accepts_admin_or_operator_only() {
    let s = setup();
    let tenant = Address::generate(&s.env);
    let record = sample_record(&s.env);
    let reason = Symbol::new(&s.env, "test_update");

    s.client
        .update_reputation(&s.admin, &tenant, &record, &reason);
    s.client
        .update_reputation(&s.operator, &tenant, &record, &reason);

    assert_eq!(
        s.client
            .try_update_reputation(&s.attacker, &tenant, &record, &reason)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "update_reputation must reject a caller that is neither admin nor operator"
    );
}

/// The operator role is narrower than admin: it must not open the admin gate.
#[test]
fn operator_does_not_pass_the_admin_gate() {
    let s = setup();
    let tenant = Address::generate(&s.env);

    assert_eq!(
        s.client
            .try_revoke_reputation(&s.operator, &tenant)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "operator must not pass the admin gate"
    );
}

/// A rejected update must leave the tenant's record untouched.
#[test]
fn rejected_update_does_not_change_reputation() {
    let s = setup();
    let tenant = Address::generate(&s.env);
    let record = sample_record(&s.env);
    let reason = Symbol::new(&s.env, "test_update");

    s.client
        .update_reputation(&s.admin, &tenant, &record, &reason);
    let before = s.client.get_reputation(&tenant).unwrap();

    let mut forged = record.clone();
    forged.composite_score = 1;
    let result = s
        .client
        .try_update_reputation(&s.attacker, &tenant, &forged, &reason);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(s.client.get_reputation(&tenant).unwrap(), before);
}

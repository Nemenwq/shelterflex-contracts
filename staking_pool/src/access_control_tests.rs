//! Authorization-boundary tests: every admin-gated entry point must reject a
//! non-admin caller.

extern crate std;

use crate::{ContractError, StakingPool, StakingPoolClient};
use soroban_pausable_core::PausableError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

struct Setup<'a> {
    env: Env,
    client: StakingPoolClient<'a>,
    admin: Address,
    attacker: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(StakingPool, ());
    let client = StakingPoolClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_admin = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    client.init(&admin, &token);

    Setup {
        env,
        client,
        admin,
        attacker,
    }
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let s = setup();
    let other = Address::generate(&s.env);
    let hash = BytesN::from_array(&s.env, &[6u8; 32]);

    assert_eq!(
        s.client
            .try_set_operator(&s.attacker, &Some(s.attacker.clone()))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_operator must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_admin(&s.attacker, &s.attacker)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_admin must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_lock_period(&s.attacker, &600)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_lock_period must reject a non-admin"
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
            .try_propose_upgrade(&s.attacker, &hash, &2)
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
            .try_emergency_upgrade(&s.attacker, &hash, &2)
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

/// A rejected call must not let an attacker install itself as admin or
/// operator, nor change the lock period.
#[test]
fn rejected_call_does_not_change_roles_or_config() {
    let s = setup();

    let lock_before = s.client.get_lock_period();

    let result = s.client.try_set_admin(&s.attacker, &s.attacker);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s
        .client
        .try_set_operator(&s.attacker, &Some(s.attacker.clone()));
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s.client.try_set_lock_period(&s.attacker, &9_999);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert!(!s.client.is_operator(&s.attacker));
    assert_eq!(s.client.get_lock_period(), lock_before);
}

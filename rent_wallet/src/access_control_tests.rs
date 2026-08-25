//! Authorization-boundary tests: every admin-gated entry point must reject a
//! non-admin caller.

extern crate std;

use crate::{ContractError, RentWallet, RentWalletClient};
use soroban_pausable_core::PausableError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

struct Setup<'a> {
    env: Env,
    client: RentWalletClient<'a>,
    admin: Address,
    attacker: Address,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(RentWallet, ());
    let client = RentWalletClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.init(&admin);

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
    let user = Address::generate(&s.env);
    let hash = BytesN::from_array(&s.env, &[5u8; 32]);

    assert_eq!(
        s.client
            .try_credit(&s.attacker, &user, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "credit must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_debit(&s.attacker, &user, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "debit must reject a non-admin"
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
            .try_set_guardian(&s.attacker, &user)
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
        s.client
            .try_set_default_monthly_cap(&s.attacker, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_default_monthly_cap must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_user_monthly_cap(&s.attacker, &user, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_user_monthly_cap must reject a non-admin"
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

/// A rejected credit/debit must leave balances exactly where they were.
#[test]
fn rejected_call_does_not_move_balances() {
    let s = setup();
    let user = Address::generate(&s.env);

    s.client.credit(&s.admin, &user, &5_000);
    let before = s.client.balance(&user);

    let result = s.client.try_credit(&s.attacker, &user, &1_000);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s.client.try_debit(&s.attacker, &user, &1_000);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(s.client.balance(&user), before);
}

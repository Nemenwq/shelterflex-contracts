//! Authorization-boundary tests: every gated entry point must reject a
//! non-admin caller, and every state mutation must be refused while paused.

extern crate std;

use super::*;
use soroban_pausable_core::PausableError;
use soroban_sdk::{testutils::Address as _, Env};

fn setup(env: &Env) -> (Address, RentToOwnClient<'_>) {
    env.mock_all_auths();
    let id = env.register(RentToOwn, ());
    let client = RentToOwnClient::new(env, &id);
    let admin = Address::generate(env);
    client.init(&admin, &2000u32);
    (admin, client)
}

fn deal_id(env: &Env, seed: u8) -> BytesN<32> {
    BytesN::from_array(env, &[seed; 32])
}

/// Register an active deal with one recorded payment, so the mutators that need
/// an existing deal have one to act on.
fn active_deal(env: &Env, admin: &Address, client: &RentToOwnClient<'_>, seed: u8) -> BytesN<32> {
    let id = deal_id(env, seed);
    let tenant = Address::generate(env);
    client.register_deal(admin, &id, &tenant, &100_000, &10_000, &10);
    client.record_equity_payment(admin, &id, &15_000, &10_000);
    id
}

// ── Non-admin is rejected on every gated entry point ─────────────────────────

#[test]
fn non_admin_rejected_on_every_gated_entry_point() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let attacker = Address::generate(&env);
    let tenant = Address::generate(&env);
    let id = active_deal(&env, &admin, &client, 1);

    let unregistered = deal_id(&env, 9);
    assert_eq!(
        client
            .try_register_deal(&attacker, &unregistered, &tenant, &100_000, &10_000, &10)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "register_deal must reject a non-admin"
    );

    assert_eq!(
        client
            .try_record_equity_payment(&attacker, &id, &15_000, &10_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "record_equity_payment must reject a non-admin"
    );

    assert_eq!(
        client
            .try_complete_deal(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "complete_deal must reject a non-admin"
    );

    assert_eq!(
        client
            .try_default_deal(&attacker, &id, &Symbol::new(&env, "reason"))
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "default_deal must reject a non-admin"
    );

    assert_eq!(
        client
            .try_settle_default(&attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "settle_default must reject a non-admin"
    );

    assert_eq!(
        client
            .try_transfer_position(&attacker, &tenant, &attacker, &id)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "transfer_position must reject a non-admin"
    );

    assert_eq!(
        client.try_pause(&attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "pause must reject a non-admin"
    );

    client.pause(&admin);
    assert_eq!(
        client.try_unpause(&attacker).unwrap_err().unwrap(),
        PausableError::NotAuthorized,
        "unpause must reject a non-admin"
    );
}

/// A rejected call must leave the deal untouched — no equity moved, no status
/// change, nothing for a caller to retry against.
#[test]
fn rejected_call_does_not_change_deal_state() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let attacker = Address::generate(&env);
    let id = active_deal(&env, &admin, &client, 2);

    let before = client.get_deal(&id).unwrap();

    let result = client.try_record_equity_payment(&attacker, &id, &15_000, &10_000);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let after = client.get_deal(&id).unwrap();
    assert_eq!(
        before.equity_accumulated_usdc,
        after.equity_accumulated_usdc
    );
    assert_eq!(before.payments_made, after.payments_made);
    assert!(matches!(after.status, DealStatus::Active));
}

// ── Pause blocks state mutation and lifts on unpause ─────────────────────────

#[test]
fn contract_starts_unpaused() {
    let env = Env::default();
    let (_admin, client) = setup(&env);
    assert!(!client.is_paused());
}

#[test]
fn admin_can_pause_and_unpause() {
    let env = Env::default();
    let (admin, client) = setup(&env);

    client.pause(&admin);
    assert!(client.is_paused());

    client.unpause(&admin);
    assert!(!client.is_paused());
}

#[test]
fn paused_contract_rejects_every_state_mutation() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let tenant = Address::generate(&env);
    let id = active_deal(&env, &admin, &client, 3);

    client.pause(&admin);

    let fresh = deal_id(&env, 4);
    assert_eq!(
        client
            .try_register_deal(&admin, &fresh, &tenant, &100_000, &10_000, &10)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "register_deal must be refused while paused"
    );

    assert_eq!(
        client
            .try_record_equity_payment(&admin, &id, &15_000, &10_000)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "record_equity_payment must be refused while paused"
    );

    assert_eq!(
        client.try_complete_deal(&admin, &id).unwrap_err().unwrap(),
        ContractError::Paused,
        "complete_deal must be refused while paused"
    );

    assert_eq!(
        client
            .try_default_deal(&admin, &id, &Symbol::new(&env, "reason"))
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "default_deal must be refused while paused"
    );

    assert_eq!(
        client.try_settle_default(&admin, &id).unwrap_err().unwrap(),
        ContractError::Paused,
        "settle_default must be refused while paused"
    );

    assert_eq!(
        client
            .try_transfer_position(&admin, &tenant, &Address::generate(&env), &id)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused,
        "transfer_position must be refused while paused"
    );
}

#[test]
fn getters_still_work_while_paused() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let id = active_deal(&env, &admin, &client, 5);

    client.pause(&admin);

    assert!(client.get_deal(&id).is_some());
    assert_eq!(client.get_equity_percentage(&id), 1_000);
}

#[test]
fn state_mutation_resumes_after_unpause() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let id = active_deal(&env, &admin, &client, 6);

    client.pause(&admin);
    assert_eq!(
        client
            .try_record_equity_payment(&admin, &id, &15_000, &10_000)
            .unwrap_err()
            .unwrap(),
        ContractError::Paused
    );

    client.unpause(&admin);
    client.record_equity_payment(&admin, &id, &15_000, &10_000);

    let deal = client.get_deal(&id).unwrap();
    assert_eq!(deal.payments_made, 2);
    assert_eq!(deal.equity_accumulated_usdc, 20_000);
}

/// Pausing must not touch the equity ledger: the numbers on either side of a
/// pause/unpause cycle are identical.
#[test]
fn pause_cycle_preserves_equity_state() {
    let env = Env::default();
    let (admin, client) = setup(&env);
    let id = active_deal(&env, &admin, &client, 7);

    let before = client.get_deal(&id).unwrap();
    client.pause(&admin);
    client.unpause(&admin);
    let after = client.get_deal(&id).unwrap();

    assert_eq!(
        before.equity_accumulated_usdc,
        after.equity_accumulated_usdc
    );
    assert_eq!(before.payments_made, after.payments_made);
    assert_eq!(before.tenant, after.tenant);
}

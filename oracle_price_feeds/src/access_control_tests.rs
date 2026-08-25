//! Authorization-boundary tests: every gated entry point must reject a caller
//! that holds neither the admin nor the operator role.

extern crate std;

use super::*;
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env, Symbol};

struct Setup<'a> {
    env: Env,
    client: OraclePriceFeedsClient<'a>,
    admin: Address,
    operator: Address,
    attacker: Address,
    pair: Symbol,
}

fn setup<'a>() -> Setup<'a> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let contract_id = env.register(OraclePriceFeeds, ());
    let client = OraclePriceFeedsClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let attacker = Address::generate(&env);
    let pair = Symbol::new(&env, "NGN_USDC");
    client.init(&admin, &operator, &600u64, &500u64);

    Setup {
        env,
        client,
        admin,
        operator,
        attacker,
        pair,
    }
}

#[test]
fn non_admin_rejected_on_every_admin_gated_entry_point() {
    let s = setup();
    let source = Address::generate(&s.env);

    assert_eq!(
        s.client
            .try_add_source(&s.attacker, &s.pair, &source)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "add_source must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_remove_source(&s.attacker, &s.pair, &source)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "remove_source must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_quorum(&s.attacker, &s.pair, &1)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_quorum must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_staleness_threshold(&s.attacker, &900)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_staleness_threshold must reject a non-admin"
    );

    assert_eq!(
        s.client
            .try_set_max_deviation_bps(&s.attacker, &1_000)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "set_max_deviation_bps must reject a non-admin"
    );
}

/// In single-source mode `update_price` accepts admin *or* operator, and
/// nobody else.
#[test]
fn update_price_accepts_admin_or_operator_only() {
    let s = setup();

    s.client.update_price(&s.admin, &s.pair, &1_000_000, &1);
    s.client.update_price(&s.operator, &s.pair, &1_000_000, &2);

    assert_eq!(
        s.client
            .try_update_price(&s.attacker, &s.pair, &1_000_000, &3)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "update_price must reject a caller that is neither admin nor operator"
    );
}

/// The operator role is narrower than admin: it must not open the admin gate.
#[test]
fn operator_does_not_pass_the_admin_gate() {
    let s = setup();
    let source = Address::generate(&s.env);

    assert_eq!(
        s.client
            .try_add_source(&s.operator, &s.pair, &source)
            .unwrap_err()
            .unwrap(),
        ContractError::NotAuthorized,
        "operator must not pass the admin gate"
    );
}

/// A rejected call must leave the feed configuration untouched — an attacker
/// must not be able to widen staleness or add itself as a price source.
#[test]
fn rejected_call_does_not_change_feed_config() {
    let s = setup();

    let sources_before = s.client.get_sources(&s.pair).len();

    let result = s.client.try_set_staleness_threshold(&s.attacker, &900);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    let result = s.client.try_add_source(&s.attacker, &s.pair, &s.attacker);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::NotAuthorized);

    assert_eq!(s.client.get_sources(&s.pair).len(), sources_before);
}

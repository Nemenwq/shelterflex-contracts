//! Ledger-time TTL tests for `rent_to_own`.
//!
//! A rent-to-own deal is written at lease start and read on every monthly
//! equity payment for 12–24 months — the longest-lived state on the platform.

use crate::{RentToOwn, RentToOwnClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

fn deal_id(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

fn setup(env: &Env) -> (RentToOwnClient<'_>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let tenant = Address::generate(env);
    let client = RentToOwnClient::new(env, &env.register(RentToOwn, ()));
    client.init(&admin, &2_000);
    (client, admin, tenant)
}

/// The canonical failure this issue describes: a tenant records month 24's
/// equity payment and the deal entry must still be there.
#[test]
fn deal_and_equity_ledger_survive_a_two_year_lease() {
    let env = mainnet_env();
    let (client, admin, tenant) = setup(&env);
    let id = deal_id(&env, 1);

    client.register_deal(&admin, &id, &tenant, &240_000, &1_000, &24);

    for month in 1..=24u32 {
        advance_ledgers(&env, MONTH);
        client.record_equity_payment(&admin, &id, &2_000, &1_000);

        let deal = client.get_deal(&id).unwrap();
        assert_eq!(deal.payments_made, month);
        assert_eq!(deal.equity_accumulated_usdc, i128::from(month) * 1_000);
    }

    // Month 24: the deal completes, two years after it was written.
    client.complete_deal(&admin, &id);
    // 24_000 of 240_000 accrued: 1_000 bps.
    assert_eq!(client.get_equity_percentage(&id), 1_000);
    let deal = client.get_deal(&id).unwrap();
    assert_eq!(deal.equity_accumulated_usdc, 24_000);
}

/// A deal read (not written) just inside the default entry lifetime is kept
/// alive by that read alone.
#[test]
fn reading_a_deal_keeps_it_alive_past_the_default_lifetime() {
    let env = mainnet_env();
    let (client, admin, tenant) = setup(&env);
    let id = deal_id(&env, 2);

    client.register_deal(&admin, &id, &tenant, &120_000, &1_000, &12);

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    assert!(client.get_deal(&id).is_some());

    // Past the ledger at which an unextended deal would have been archived.
    advance_ledgers(&env, MONTH * 3);
    assert!(client.get_deal(&id).is_some());
    client.record_equity_payment(&admin, &id, &2_000, &1_000);
    assert_eq!(client.get_deal(&id).unwrap().payments_made, 1);
}

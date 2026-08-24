//! Ledger-time TTL tests for `whistleblower_rewards`.
//!
//! An allocation is written when a report is accepted and may sit unclaimed
//! through the hold window and well beyond it — persistent state that must not
//! archive out from under the whistleblower.

use crate::{WhistleblowerRewards, WhistleblowerRewardsClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env, String};
use soroban_storage_ttl::testutils::{
    advance_ledgers, keep_contract_alive, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

struct Fixture<'a> {
    client: WhistleblowerRewardsClient<'a>,
    token: Address,
    operator: Address,
    whistleblower: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let operator = Address::generate(env);
    let whistleblower = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let contract_id = env.register(WhistleblowerRewards, ());
    let client = WhistleblowerRewardsClient::new(env, &contract_id);
    // No hold window: the tests here are about entry lifetimes, not vesting.
    client.init(&admin, &operator, &token, &0);

    StellarAssetClient::new(env, &token).mint(&contract_id, &1_000_000);

    Fixture {
        client,
        token,
        operator,
        whistleblower,
    }
}

/// Allocations accrued monthly for two years are all still claimable at the
/// end — reading `claimable` walks every allocation record and extends each.
#[test]
fn allocations_survive_a_two_year_reporting_history() {
    let env = mainnet_env();
    let f = setup(&env);
    let listing = String::from_str(&env, "listing-ttl-1");

    for month in 1..=24i128 {
        advance_ledgers(&env, MONTH);
        keep_contract_alive(&env, &f.token);

        f.client.allocate(
            &f.operator,
            &f.whistleblower,
            &listing,
            &String::from_str(&env, "deal-ttl-1"),
            &1_000,
        );
        assert_eq!(
            f.client.claimable(&f.whistleblower, &listing),
            month * 1_000
        );
    }

    let claimed = f.client.claim(&f.whistleblower, &listing, &None);
    assert_eq!(claimed, 24_000);
    assert_eq!(f.client.claimable(&f.whistleblower, &listing), 0);
}

/// An allocation left unclaimed is kept alive by nothing more than the
/// whistleblower checking their balance.
#[test]
fn unclaimed_allocation_outlives_the_default_entry_lifetime() {
    let env = mainnet_env();
    let f = setup(&env);
    let listing = String::from_str(&env, "listing-ttl-2");

    f.client.allocate(
        &f.operator,
        &f.whistleblower,
        &listing,
        &String::from_str(&env, "deal-ttl-2"),
        &3_000,
    );

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.claimable(&f.whistleblower, &listing), 3_000);

    // Past the ledger at which an unextended allocation would have been
    // archived — and the reward is still payable.
    advance_ledgers(&env, MONTH * 3);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.claimable(&f.whistleblower, &listing), 3_000);
    assert_eq!(f.client.claim(&f.whistleblower, &listing, &None), 3_000);
}

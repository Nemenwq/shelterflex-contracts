//! Ledger-time TTL tests for `mvp_staking_pool`.
//!
//! Stake balances and per-user reward checkpoints live in persistent storage;
//! `TotalStaked` and the global reward index live in instance storage, which
//! takes the whole contract with it if it archives.

use crate::{StakingPool, StakingPoolClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{Address, Env};
use soroban_storage_ttl::testutils::{
    advance_ledgers, keep_contract_alive, mainnet_env, MAINNET_MIN_PERSISTENT_TTL, MONTH,
};

struct Fixture<'a> {
    client: StakingPoolClient<'a>,
    token: Address,
    admin: Address,
    staker: Address,
}

fn setup(env: &Env) -> Fixture<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let staker = Address::generate(env);
    let token_admin = Address::generate(env);
    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();

    let client = StakingPoolClient::new(env, &env.register(StakingPool, ()));
    client.init(&admin, &token);

    let token_client = StellarAssetClient::new(env, &token);
    token_client.mint(&staker, &1_000_000);
    token_client.mint(&admin, &1_000_000);

    Fixture {
        client,
        token,
        admin,
        staker,
    }
}

/// A stake plus its reward checkpoint survives a 24-month lease of monthly
/// reward funding, and the rewards are still claimable at the end.
#[test]
fn stake_and_rewards_survive_a_two_year_lease() {
    let env = mainnet_env();
    let f = setup(&env);

    f.client.stake(&f.staker, &10_000);

    for _ in 0..24 {
        advance_ledgers(&env, MONTH);
        keep_contract_alive(&env, &f.token);

        f.client.fund_rewards(&f.admin, &1_000);
        // Reading the claimable amount accrues and re-writes the user's reward
        // checkpoint, extending it.
        assert!(f.client.claimable(&f.staker) > 0);
    }

    assert_eq!(f.client.staked_balance(&f.staker), 10_000);
    assert_eq!(f.client.total_staked(), 10_000);

    let claimed = f.client.claim(&f.staker);
    assert_eq!(claimed, 24_000);
    assert_eq!(f.client.claimable(&f.staker), 0);
}

/// A stake that is only read is kept alive by the read, and the contract
/// instance (which holds `TotalStaked`) is kept alive by the call itself.
#[test]
fn reading_a_stake_keeps_it_alive_past_the_default_lifetime() {
    let env = mainnet_env();
    let f = setup(&env);

    f.client.stake(&f.staker, &4_000);

    advance_ledgers(&env, MAINNET_MIN_PERSISTENT_TTL - MONTH);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.staked_balance(&f.staker), 4_000);

    // Past the ledger at which an unextended entry would have been archived.
    advance_ledgers(&env, MONTH * 3);
    keep_contract_alive(&env, &f.token);
    assert_eq!(f.client.staked_balance(&f.staker), 4_000);
    assert_eq!(f.client.total_staked(), 4_000);

    f.client.stake(&f.staker, &1_000);
    assert_eq!(f.client.staked_balance(&f.staker), 5_000);
}

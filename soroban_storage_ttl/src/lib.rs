#![no_std]
//! Shared storage time-to-live (TTL) policy for the Shelterflex contracts.
//!
//! # Why this crate exists
//!
//! Soroban storage is not permanent. Every `persistent` and `instance` entry
//! carries a *live-until ledger*; once that ledger passes the entry is archived
//! and can no longer be read or written until somebody submits a
//! `RestoreFootprint` operation for it. `temporary` entries are worse still —
//! when their TTL lapses they are **deleted**, with no restore path.
//!
//! The default lifetimes are network parameters, they are finite (weeks, not
//! years), and the SDK does not extend anything on your behalf. A contract that
//! never calls `extend_ttl` has therefore chosen the default clock, which for a
//! rent-financing platform is the wrong clock: a `rent_to_own` deal is read on
//! every monthly payment for 12–24 months, an escrow deposit sits untouched
//! between funding and settlement, and a staking position outlives both.
//!
//! Instance storage is the sharper edge: it shares the contract instance's TTL,
//! so if it lapses the *whole contract* is archived and every entrypoint fails
//! until the instance is restored.
//!
//! # The policy
//!
//! Ledgers close roughly every 5 seconds on Stellar mainnet, so one day is
//! about [`LEDGERS_PER_DAY`] ledgers. Every threshold below is expressed in
//! those units:
//!
//! | Storage class | Bump when TTL below     | Extend to           |
//! |---------------|-------------------------|---------------------|
//! | instance      | 90 days                 | 120 days            |
//! | persistent    | 90 days                 | 120 days            |
//! | temporary     | 15 days                 | 30 days             |
//!
//! ## Why 120 days, and why the same number everywhere
//!
//! * **It is under the network cap.** Mainnet's `max_entry_ttl` is 3_110_400
//!   ledgers (~180 days). An extend-to target above the cap is clamped for
//!   persistent/instance entries and is an *error* for temporary ones, so the
//!   policy stays comfortably below it and behaves identically on every
//!   network.
//! * **It covers a 12–24 month lease without a single restore.** No single
//!   `extend_ttl` can reach 24 months (the cap forbids it), so long-lived state
//!   has to be kept alive by being used. The platform's long-lived entries are
//!   touched at least monthly (rent payment, equity accrual, reward accrual),
//!   and 120 days is a 4x margin over that 30-day cadence: three consecutive
//!   missed months still leave the entry live.
//! * **The threshold is not zero.** Extending only below 90 days means a hot
//!   entry touched many times a day pays the `extend_ttl` write cost at most
//!   once per 30 days of ledger time, instead of on every access.
//! * **One number per class, workspace-wide.** Divergent per-contract values
//!   are how this rots: a deal in `deal_escrow` and its receipt in
//!   `transaction-receipt-contract` must not archive on different days.
//!
//! Temporary storage gets a deliberately shorter window because entries that
//! belong there are short-lived by definition — and because a *deleted* entry
//! is unrecoverable, nothing that represents money or an obligation may live
//! there in the first place.
//!
//! # How to use it
//!
//! Bring [`TtlStorage`] into scope and go through it instead of touching
//! `env.storage().persistent()` / `.temporary()` directly:
//!
//! ```ignore
//! use soroban_storage_ttl::TtlStorage;
//!
//! pub fn pay(env: Env, deal_id: String, amount: i128) {
//!     env.extend_instance_ttl(); // first line of every entrypoint
//!
//!     let deal: Deal = env.get_persistent(&DataKey::Deal(deal_id.clone())).unwrap();
//!     env.set_persistent(&DataKey::Deal(deal_id), &deal);
//! }
//! ```
//!
//! Every accessor here extends the entry's TTL as a side effect, so an entry
//! that is read regularly is kept alive by being read.

use soroban_sdk::{Env, IntoVal, TryFromVal, Val};

/// Ledgers closed in a day at mainnet's ~5 second ledger close time.
pub const LEDGERS_PER_DAY: u32 = 17_280;

/// Extend the contract instance when its TTL drops below 90 days.
pub const INSTANCE_BUMP_THRESHOLD: u32 = 90 * LEDGERS_PER_DAY;
/// Extend the contract instance out to 120 days.
pub const INSTANCE_BUMP_TO: u32 = 120 * LEDGERS_PER_DAY;

/// Extend a persistent entry when its TTL drops below 90 days.
pub const PERSISTENT_BUMP_THRESHOLD: u32 = 90 * LEDGERS_PER_DAY;
/// Extend a persistent entry out to 120 days.
pub const PERSISTENT_BUMP_TO: u32 = 120 * LEDGERS_PER_DAY;

/// Extend a temporary entry when its TTL drops below 15 days.
pub const TEMPORARY_BUMP_THRESHOLD: u32 = 15 * LEDGERS_PER_DAY;
/// Extend a temporary entry out to 30 days.
pub const TEMPORARY_BUMP_TO: u32 = 30 * LEDGERS_PER_DAY;

/// Storage accessors that apply the workspace TTL policy on every access.
///
/// Implemented for [`Env`], so call sites read as `env.set_persistent(..)` and
/// work whether the surrounding code holds an `Env` or an `&Env`.
pub trait TtlStorage {
    /// Extend the contract instance (and everything in instance storage) out to
    /// [`INSTANCE_BUMP_TO`]. Call this at the top of every public entrypoint.
    fn extend_instance_ttl(&self);

    /// Extend a single persistent entry out to [`PERSISTENT_BUMP_TO`].
    ///
    /// Does nothing when the entry does not exist — extending a missing entry
    /// is a host error.
    fn extend_persistent_ttl<K>(&self, key: &K)
    where
        K: IntoVal<Env, Val>;

    /// `env.storage().persistent().set(..)` plus a TTL extension.
    fn set_persistent<K, V>(&self, key: &K, value: &V)
    where
        K: IntoVal<Env, Val>,
        V: IntoVal<Env, Val>;

    /// `env.storage().persistent().get(..)`, extending the entry's TTL when it
    /// is present — a regularly read entry is kept alive by being read.
    fn get_persistent<K, V>(&self, key: &K) -> Option<V>
    where
        V::Error: core::fmt::Debug,
        K: IntoVal<Env, Val>,
        V: TryFromVal<Env, Val>;

    /// `env.storage().persistent().has(..)`, extending the entry's TTL when it
    /// is present.
    fn has_persistent<K>(&self, key: &K) -> bool
    where
        K: IntoVal<Env, Val>;

    /// Extend a single temporary entry out to [`TEMPORARY_BUMP_TO`].
    ///
    /// Does nothing when the entry does not exist.
    fn extend_temporary_ttl<K>(&self, key: &K)
    where
        K: IntoVal<Env, Val>;

    /// `env.storage().temporary().set(..)` plus a TTL extension.
    fn set_temporary<K, V>(&self, key: &K, value: &V)
    where
        K: IntoVal<Env, Val>,
        V: IntoVal<Env, Val>;

    /// `env.storage().temporary().get(..)`, extending the entry's TTL when it
    /// is present.
    fn get_temporary<K, V>(&self, key: &K) -> Option<V>
    where
        V::Error: core::fmt::Debug,
        K: IntoVal<Env, Val>,
        V: TryFromVal<Env, Val>;

    /// `env.storage().temporary().has(..)`, extending the entry's TTL when it
    /// is present.
    fn has_temporary<K>(&self, key: &K) -> bool
    where
        K: IntoVal<Env, Val>;
}

impl TtlStorage for Env {
    fn extend_instance_ttl(&self) {
        self.storage()
            .instance()
            .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_TO);
    }

    fn extend_persistent_ttl<K>(&self, key: &K)
    where
        K: IntoVal<Env, Val>,
    {
        let storage = self.storage().persistent();
        if storage.has(key) {
            storage.extend_ttl(key, PERSISTENT_BUMP_THRESHOLD, PERSISTENT_BUMP_TO);
        }
    }

    fn set_persistent<K, V>(&self, key: &K, value: &V)
    where
        K: IntoVal<Env, Val>,
        V: IntoVal<Env, Val>,
    {
        let storage = self.storage().persistent();
        storage.set(key, value);
        storage.extend_ttl(key, PERSISTENT_BUMP_THRESHOLD, PERSISTENT_BUMP_TO);
    }

    fn get_persistent<K, V>(&self, key: &K) -> Option<V>
    where
        V::Error: core::fmt::Debug,
        K: IntoVal<Env, Val>,
        V: TryFromVal<Env, Val>,
    {
        let storage = self.storage().persistent();
        let value = storage.get(key);
        if value.is_some() {
            storage.extend_ttl(key, PERSISTENT_BUMP_THRESHOLD, PERSISTENT_BUMP_TO);
        }
        value
    }

    fn has_persistent<K>(&self, key: &K) -> bool
    where
        K: IntoVal<Env, Val>,
    {
        let storage = self.storage().persistent();
        let present = storage.has(key);
        if present {
            storage.extend_ttl(key, PERSISTENT_BUMP_THRESHOLD, PERSISTENT_BUMP_TO);
        }
        present
    }

    fn extend_temporary_ttl<K>(&self, key: &K)
    where
        K: IntoVal<Env, Val>,
    {
        let storage = self.storage().temporary();
        if storage.has(key) {
            storage.extend_ttl(key, TEMPORARY_BUMP_THRESHOLD, TEMPORARY_BUMP_TO);
        }
    }

    fn set_temporary<K, V>(&self, key: &K, value: &V)
    where
        K: IntoVal<Env, Val>,
        V: IntoVal<Env, Val>,
    {
        let storage = self.storage().temporary();
        storage.set(key, value);
        storage.extend_ttl(key, TEMPORARY_BUMP_THRESHOLD, TEMPORARY_BUMP_TO);
    }

    fn get_temporary<K, V>(&self, key: &K) -> Option<V>
    where
        V::Error: core::fmt::Debug,
        K: IntoVal<Env, Val>,
        V: TryFromVal<Env, Val>,
    {
        let storage = self.storage().temporary();
        let value = storage.get(key);
        if value.is_some() {
            storage.extend_ttl(key, TEMPORARY_BUMP_THRESHOLD, TEMPORARY_BUMP_TO);
        }
        value
    }

    fn has_temporary<K>(&self, key: &K) -> bool
    where
        K: IntoVal<Env, Val>,
    {
        let storage = self.storage().temporary();
        let present = storage.has(key);
        if present {
            storage.extend_ttl(key, TEMPORARY_BUMP_THRESHOLD, TEMPORARY_BUMP_TO);
        }
        present
    }
}

#[cfg(any(test, feature = "testutils"))]
pub mod testutils;

#[cfg(test)]
mod test;

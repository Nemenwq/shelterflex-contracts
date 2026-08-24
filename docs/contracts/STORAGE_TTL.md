# Storage TTL policy

Soroban storage is not permanent. Every `persistent` and `instance` entry carries a
*live-until ledger*; once it passes, the entry is **archived** and cannot be read or
written again until someone submits a `RestoreFootprint` operation. `temporary`
entries are worse: when their TTL lapses they are **deleted**, with no restore path.

Instance storage is the sharp edge: it shares the contract instance's TTL, so if it
lapses the *whole contract* is archived and every entrypoint fails until the instance
is restored.

Nothing in the SDK extends anything on your behalf. A contract that never calls
`extend_ttl` has chosen the network's default clock — which, for a platform whose
deals run 12–24 months, is the wrong clock.

## The policy

One policy, one place: [`soroban_storage_ttl`](../../soroban_storage_ttl/src/lib.rs).

Ledgers close about every 5 seconds on mainnet, so a day is `LEDGERS_PER_DAY = 17_280`
ledgers.

| Storage class | Bump when TTL below | Extend to | Constants |
|---------------|---------------------|-----------|-----------|
| instance      | 90 days (1_555_200) | 120 days (2_073_600) | `INSTANCE_BUMP_THRESHOLD` / `INSTANCE_BUMP_TO` |
| persistent    | 90 days (1_555_200) | 120 days (2_073_600) | `PERSISTENT_BUMP_THRESHOLD` / `PERSISTENT_BUMP_TO` |
| temporary     | 15 days (259_200)   | 30 days (518_400)    | `TEMPORARY_BUMP_THRESHOLD` / `TEMPORARY_BUMP_TO` |

### Why these numbers

* **Under the network cap.** Mainnet's `max_entry_ttl` is 3_110_400 ledgers (~180
  days). Extending past the cap is clamped for persistent/instance entries and is an
  outright error for temporary ones, so the targets stay below it and behave the same
  on every network. This is asserted at compile time in the crate's tests.
* **Covers a 12–24 month lease.** No single `extend_ttl` can reach 24 months — the cap
  forbids it — so long-lived state is kept alive *by being used*. The platform's
  long-lived entries are touched at least monthly (rent payment, equity accrual,
  reward accrual), and 120 days is a 4× margin over that 30-day cadence: three
  consecutive missed months still leave the entry live.
* **The threshold is not zero.** Extending only below 90 days means a hot entry pays
  the `extend_ttl` write cost at most once per 30 days of ledger time rather than on
  every access.
* **One number per class, workspace-wide.** Divergent per-contract values are how this
  rots: a deal in `deal_escrow` and its receipt in `transaction-receipt-contract` must
  not archive on different days.

## How contracts use it

`soroban_storage_ttl::TtlStorage` is implemented for `Env`. Bring it into scope and go
through it instead of touching `env.storage().persistent()` / `.temporary()` directly:

```rust
use soroban_storage_ttl::TtlStorage;

pub fn record_equity_payment(env: Env, deal_id: BytesN<32>, amount: i128) -> Result<(), ContractError> {
    env.extend_instance_ttl();                       // first line of every entrypoint

    let mut deal: Deal = env.get_persistent(&DataKey::Deal(deal_id.clone()))   // read extends
        .ok_or(ContractError::DealNotFound)?;
    deal.equity += amount;
    env.set_persistent(&DataKey::Deal(deal_id), &deal);                        // write extends
    Ok(())
}
```

| Call | Behaviour |
|------|-----------|
| `env.extend_instance_ttl()` | Extends the contract instance. Call it first in every public entrypoint. |
| `env.set_persistent(&key, &value)` | Writes, then extends that entry. |
| `env.get_persistent(&key)` | Reads, and extends the entry when it is present. |
| `env.has_persistent(&key)` | Checks, and extends the entry when it is present. |
| `env.extend_persistent_ttl(&key)` | Extends an entry you touched some other way; a no-op when it does not exist. |
| `env.set_temporary` / `get_temporary` / `has_temporary` | The same, on the shorter temporary window. |

`remove` is deliberately not wrapped: a deleted entry has no TTL to manage.

## What is covered

Every stateful contract in the workspace: 382 public entrypoints extend the instance
TTL, and every persistent read/write in contract code goes through the policy helpers.
The money-holding contracts — `deal_escrow`, `rent_wallet`, `rent_payments`,
`rent_to_own`, `staking_pool`, `mvp_staking_pool`, `bond_collateral`,
`whistleblower_rewards`, `transaction-receipt-contract` — each carry a `ttl_tests`
module that runs against mainnet's archival settings and advances the ledger past the
default 120-day lifetime.

Test helpers live behind the crate's `testutils` feature
(`mainnet_env`, `advance_ledgers`, `keep_contract_alive`, `MONTH`, …), so every TTL
test uses the same ledger arithmetic. A test that never advances the ledger cannot
catch an archival bug; the crate keeps a negative control
(`unmanaged_entry_is_archived_after_the_default_lifetime`) that fails if the test
ledger ever stops archiving.

## Storage-class notes

Nothing was moved between storage classes in the change that introduced this policy —
the classes that exist are documented here instead, per the audit brief.

* **`timelock` queues transactions in `temporary` storage** (`DataKey::Queued(hash)`).
  Temporary entries are *deleted*, not archived, and the network default is ~24 hours,
  so before this policy any queued action with a delay longer than a day could vanish
  from the queue. The policy now extends those entries to 30 days on write and on read,
  which covers the 14-day `GRACE_PERIOD` plus a two-week delay. It does **not** cover an
  arbitrarily large `MaxDelay`: a timelock configured with a delay beyond ~16 days can
  still lose its queue entry. The durable fix is to move governance queue entries to
  `persistent` storage (or to bound `MaxDelay + GRACE_PERIOD` below the temporary
  target) — a behaviour change, so it belongs in its own PR.
* **No recoverable financial state lives in `temporary` storage.** Escrow balances,
  deal state, staking positions, bonds, allocations and receipts are all `persistent`;
  admin/operator/token/pause/threshold configuration is `instance`. Those classes are
  correct as they stand.

## Known limits

Extend-on-access keeps alive exactly what a call touches. Two consequences are worth
knowing before relying on this:

1. **An entry nothing touches for 120 days still archives.** For state that is expected
   to sit idle longer than that — an escrow deposit parked between funding and
   settlement — something must touch it inside the window, or it will need a restore.
   A `RestoreFootprint` recovery path is deliberately out of scope here.
2. **Sibling keys of one logical record are only extended if they are actually read.**
   Two known cases:
   * `staking_pool` writes one `Deposit(user, n)` entry per stake and only `unstake`
     reads them. A staker who neither stakes nor unstakes for 120 days will find those
     records archived, and the deposit counter with them.
   * `rent_payments` writes a `UsedReference(deal, ref)` marker per payment that is only
     read when that same reference is replayed. Old markers archive; a replay of one of
     them then fails the call rather than returning `DuplicateReference`.

   Both need either a deliberate touch (with the gas cost that implies) or a restore
   path, and both are behaviour changes rather than TTL management.

#![no_std]
//! Shared in-contract access control primitives.
//!
//! **This crate is the workspace standard for admin/operator gating.** Every contract
//! that guards an entry point on an admin or operator role calls these primitives
//! directly — there are no per-contract copies. A reviewer can audit the gate once
//! here, and `grep -rn "soroban_access_control_core::"` enumerates every gated path
//! in the tree, with the function name stating which strength of check applies.
//!
//! These are *linked* helpers, not cross-contract calls: a contract that guards a
//! path calls into this module directly, so the guard costs one instance-storage
//! read/write and never leaves the contract. That is deliberate — routing the check
//! through the deployable `soroban_access_control` contract would put an external call
//! inside the guard itself, which is both more gas and a weaker security model.
//!
//! This is a **library-only** crate (`crate-type = ["rlib"]`, no `#[contract]`). The
//! primitives deliberately do not live alongside the `soroban_access_control` contract:
//! the workspace release profile uses fat LTO, so linking a crate that carries
//! `#[contractimpl]` entry points pulls those exports into the dependent contract's
//! WASM and fails the build with `Linking globals named 'init': symbol multiply
//! defined!`. Keep this crate free of contract entry points.
//!
//! That constraint is why the deployable `soroban_access_control` contract cannot be
//! the crate contracts import: it exists as the on-chain reference implementation and
//! conformance suite for these primitives, and links this crate like everyone else.

use soroban_sdk::{Address, Env, Symbol};

#[cfg(kani)]
mod formal_properties;

#[cfg(kani)]
pub use formal_properties::*;

/// Emit a standardized unauthorized-access event and return the provided contract error.
#[inline]
pub fn deny<E>(env: &Env, caller: &Address, operation: &str, err: E) -> E {
    env.events().publish(
        (
            Symbol::new(env, "access_control"),
            Symbol::new(env, "unauthorized"),
            caller.clone(),
        ),
        Symbol::new(env, operation),
    );
    err
}

/// Require that `caller` is the current `admin`.
#[inline]
pub fn require_admin_permission<E: Copy>(
    env: &Env,
    admin: &Address,
    caller: &Address,
    operation: &str,
    not_authorized: E,
) -> Result<(), E> {
    caller.require_auth();
    if caller != admin {
        return Err(deny(env, caller, operation, not_authorized));
    }
    Ok(())
}

/// Require that `caller` is either `admin` OR an optional operator.
#[inline]
pub fn require_admin_or_operator_permission<E: Copy>(
    env: &Env,
    admin: &Address,
    operator: Option<&Address>,
    caller: &Address,
    operation: &str,
    not_authorized: E,
) -> Result<(), E> {
    caller.require_auth();
    if caller == admin {
        return Ok(());
    }
    if let Some(op) = operator {
        if caller == op {
            return Ok(());
        }
    }

    Err(deny(env, caller, operation, not_authorized))
}

/// Require that the stored `admin` has authorized this invocation.
///
/// This is the weaker, *single-admin* gate, for entry points that do not accept a
/// caller address: there is nothing to compare the caller against, so the guard is
/// the host-level auth requirement on the stored admin itself. It is equivalent to
/// `require_admin_permission` where caller == admin, and therefore cannot fail —
/// an unauthorized invocation is rejected by the host before the body runs, so no
/// `unauthorized` event is emitted.
///
/// Prefer [`require_admin_permission`] for any new entry point: taking the caller
/// explicitly makes the denial observable on-chain. Use this only to preserve the
/// ABI of an entry point that already omits the caller argument.
#[inline]
pub fn require_admin_auth(admin: &Address) {
    admin.require_auth();
}

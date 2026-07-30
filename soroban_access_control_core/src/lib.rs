#![no_std]
//! Shared in-contract access control primitives.
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

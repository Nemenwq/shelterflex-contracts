#![no_std]

use soroban_access_control_core::{require_admin_or_operator_permission, require_admin_permission};
use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};
use soroban_storage_ttl::TtlStorage;

#[cfg(kani)]
pub mod formal_properties;

// ── Test Harness Contract ─────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TestError {
    NotAuthorized = 4001,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Operator,
}

#[contract]
pub struct TestAccessControlContract;

#[contractimpl]
impl TestAccessControlContract {
    pub fn init(env: Env, admin: Address) {
        env.extend_instance_ttl();

        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    pub fn set_operator(env: Env, caller: Address, operator: Address) -> Result<(), TestError> {
        env.extend_instance_ttl();

        let admin = Self::get_admin(&env);
        require_admin_permission(
            &env,
            &admin,
            &caller,
            "set_operator",
            TestError::NotAuthorized,
        )?;
        env.storage().instance().set(&DataKey::Operator, &operator);
        Ok(())
    }

    pub fn admin_only_operation(env: Env, caller: Address) -> Result<(), TestError> {
        env.extend_instance_ttl();

        let admin = Self::get_admin(&env);
        require_admin_permission(
            &env,
            &admin,
            &caller,
            "admin_only_operation",
            TestError::NotAuthorized,
        )?;
        Ok(())
    }

    pub fn admin_or_operator_operation(env: Env, caller: Address) -> Result<(), TestError> {
        env.extend_instance_ttl();

        let admin = Self::get_admin(&env);
        let operator = Self::get_operator(&env);
        require_admin_or_operator_permission(
            &env,
            &admin,
            operator.as_ref(),
            &caller,
            "admin_or_operator_operation",
            TestError::NotAuthorized,
        )?;
        Ok(())
    }

    fn get_admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set")
    }

    fn get_operator(env: &Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::Operator)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{TestAccessControlContract, TestAccessControlContractClient, TestError};
    use soroban_sdk::testutils::{Address as _, Events, MockAuth, MockAuthInvoke};
    use soroban_sdk::{Address, Env, IntoVal};

    fn setup(env: &Env) -> (Address, TestAccessControlContractClient<'_>) {
        let contract_id = env.register(TestAccessControlContract, ());
        let client = TestAccessControlContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.init(&admin);
        (admin, client)
    }

    #[test]
    fn admin_passes_admin_permission_check() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_only_operation",
                args: (admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_only_operation(&admin);
        assert!(result.is_ok(), "admin should pass admin permission check");
    }

    #[test]
    fn non_admin_denied_admin_permission_check() {
        let env = Env::default();
        let (_admin, client) = setup(&env);

        let attacker = Address::generate(&env);

        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_only_operation",
                args: (attacker.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_only_operation(&attacker);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TestError::NotAuthorized,
            "non-admin should be denied"
        );
    }

    #[test]
    fn operator_not_mistaken_for_admin() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        let operator = Address::generate(&env);

        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "set_operator",
                args: (admin.clone(), operator.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        client.try_set_operator(&admin, &operator).unwrap().unwrap();

        env.mock_auths(&[MockAuth {
            address: &operator,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_only_operation",
                args: (operator.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_only_operation(&operator);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TestError::NotAuthorized,
            "operator should not pass admin-only check"
        );
    }

    #[test]
    fn admin_passes_admin_or_operator_permission_check() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_or_operator_operation",
                args: (admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_or_operator_operation(&admin);
        assert!(
            result.is_ok(),
            "admin should pass admin_or_operator permission check"
        );
    }

    #[test]
    fn operator_passes_admin_or_operator_permission_check() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        let operator = Address::generate(&env);

        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "set_operator",
                args: (admin.clone(), operator.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        client.try_set_operator(&admin, &operator).unwrap().unwrap();

        env.mock_auths(&[MockAuth {
            address: &operator,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_or_operator_operation",
                args: (operator.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_or_operator_operation(&operator);
        assert!(
            result.is_ok(),
            "operator should pass admin_or_operator permission check"
        );
    }

    #[test]
    fn non_admin_non_operator_denied_admin_or_operator_permission_check() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        let operator = Address::generate(&env);
        let attacker = Address::generate(&env);

        env.mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "set_operator",
                args: (admin.clone(), operator.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }]);
        client.try_set_operator(&admin, &operator).unwrap().unwrap();

        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_or_operator_operation",
                args: (attacker.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_or_operator_operation(&attacker);
        assert_eq!(
            result.unwrap_err().unwrap(),
            TestError::NotAuthorized,
            "non-admin/non-operator should be denied"
        );
    }

    #[test]
    fn unauthorized_access_emits_expected_event() {
        let env = Env::default();
        let (admin, client) = setup(&env);

        let attacker = Address::generate(&env);

        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "admin_only_operation",
                args: (attacker.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }]);

        let result = client.try_admin_only_operation(&attacker);
        assert_eq!(result.unwrap_err().unwrap(), TestError::NotAuthorized);

        let events = env.events().all();
        assert!(!events.is_empty(), "should emit unauthorized access event");
    }
}

use admin_controlled::Mask;
use near_contract_standards::fungible_token::events::FtTransfer;
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC,
};
use near_contract_standards::fungible_token::receiver::*;
use near_contract_standards::fungible_token::resolver::*;
use near_contract_standards::fungible_token::FungibleToken;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::{Base64VecU8, U128};
use near_sdk::require;
use near_sdk::{
    assert_one_yocto, env, ext_contract, near_bindgen, AccountId, Balance, Gas, PanicOnDefault,
    Promise, PromiseOrValue, StorageUsage,
};

mod ft_internal;
mod macros;

const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas(5_000_000_000_000);
const GAS_FOR_FT_TRANSFER_CALL: Gas = Gas(25_000_000_000_000 + GAS_FOR_RESOLVE_TRANSFER.0);

/// Gas to call finish withdraw method on factory.
const FINISH_WITHDRAW_GAS: Gas = Gas(Gas::ONE_TERA.0 * 50);

const DECIMALS: Balance = 9;
const MAX_UINT128: Balance = u128::MAX;
const INITIAL_AMPL_SUPPLY: Balance = 50 * 10 ^ 6 * 10 ^ DECIMALS;

// TOTAL_GONS is a multiple of INITIAL_AMPL_SUPPLY so that gons_per_ampl is an integer.
const TOTAL_GONS: Balance = MAX_UINT128 - (MAX_UINT128 % INITIAL_AMPL_SUPPLY);

// MAX_SUPPLY = maximum integer < (sqrt(4*TOTAL_GONS + 1) - 1) / 2
const MAX_SUPPLY: Balance = u128::MAX;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct BridgeToken {
    controller: AccountId,
    token: FungibleToken,
    name: String,
    symbol: String,
    reference: String,
    reference_hash: Base64VecU8,
    gons_accounts: LookupMap<AccountId, Balance>,
    gons_per_ampl: Balance,
    gons_supply: Balance,
    global_ampl_supply: Balance,
    decimals: u8,
    paused: Mask,
    #[cfg(feature = "migrate_icon")]
    icon: Option<String>,
}

const PAUSE_WITHDRAW: Mask = 1 << 0;
const PAUSE_REBASE: Mask = 1 << 1;

#[ext_contract(ext_bridge_token_factory)]
pub trait ExtBridgeTokenFactory {
    #[result_serializer(borsh)]
    fn finish_withdraw(
        &self,
        #[serializer(borsh)] amount: Balance,
        #[serializer(borsh)] recipient: AccountId,
    ) -> Promise;
}

#[near_bindgen]
impl BridgeToken {
    #[init]
    pub fn new(_global_ampl_supply: Balance) -> Self {
        assert!(!env::state_exists(), "Already initialized");

        let mut this = Self {
            controller: env::predecessor_account_id(),
            token: FungibleToken::new(b"t".to_vec()),
            name: String::default(),
            symbol: String::default(),
            reference: String::default(),
            reference_hash: Base64VecU8(vec![]),
            gons_accounts: LookupMap::new(b"m"),
            gons_per_ampl: 0,
            gons_supply: 0,
            global_ampl_supply: _global_ampl_supply,
            decimals: 0,
            paused: Mask::default(),
            #[cfg(feature = "migrate_icon")]
            icon: None,
        };

        this.internal_update_scaler();
        this
    }

    pub fn set_metadata(
        &mut self,
        name: Option<String>,
        symbol: Option<String>,
        reference: Option<String>,
        reference_hash: Option<Base64VecU8>,
        decimals: Option<u8>,
        icon: Option<String>,
    ) {
        // Only owner can change the metadata
        assert!(self.controller_or_self());

        name.map(|name| self.name = name);
        symbol.map(|symbol| self.symbol = symbol);
        reference.map(|reference| self.reference = reference);
        reference_hash.map(|reference_hash| self.reference_hash = reference_hash);
        decimals.map(|decimals| self.decimals = decimals);
        #[cfg(feature = "migrate_icon")]
        icon.map(|icon| self.icon = Some(icon));
        #[cfg(not(feature = "migrate_icon"))]
        icon.map(|_| {
            env::log("Icon was provided, but it's not supported for the token".as_bytes())
        });
    }

    #[payable]
    pub fn mint(&mut self, account_id: AccountId, amount: Balance) {
        assert_eq!(
            env::predecessor_account_id(),
            self.controller,
            "Only controller can call mint"
        );
        assert!(
            self.token.total_supply <= self.global_ampl_supply,
            "wAMPL exceed total supply"
        );
        assert!(
            self.token.total_supply <= MAX_SUPPLY,
            "wAMPL exceed maximum supply"
        );

        self.internal_deposit(&account_id, amount.into());
    }

    #[payable]
    pub fn rebase(&mut self, epoch: U128, new_total_supply: Balance) {
        self.check_not_paused(PAUSE_REBASE);
        assert_eq!(
            env::predecessor_account_id(),
            self.controller,
            "Only controller can call rebase"
        );
        assert_one_yocto();

        self.ft_rebase(epoch, new_total_supply);
    }

    #[payable]
    pub fn withdraw(&mut self, amount: U128, recipient: String) -> Promise {
        self.check_not_paused(PAUSE_WITHDRAW);

        assert_one_yocto();
        Promise::new(env::predecessor_account_id()).transfer(1);

        self.token
            .internal_withdraw(&env::predecessor_account_id(), amount.into());

        ext_bridge_token_factory::ext(self.controller.clone())
            .with_static_gas(FINISH_WITHDRAW_GAS)
            .finish_withdraw(amount.into(), recipient.parse().unwrap())
    }

    pub fn account_storage_usage(&self) -> StorageUsage {
        self.token.account_storage_usage
    }

    /// Return true if the caller is either controller or self
    pub fn controller_or_self(&self) -> bool {
        let caller = env::predecessor_account_id();
        caller == self.controller || caller == env::current_account_id()
    }
}

impl BridgeToken {
    /// Provide near rebase without dampening effect.
    /// This creates an opportunity for a cross-chain arbitrage of the rebase token.
    fn ft_rebase(&mut self, _epoch: U128, new_total_supply: Balance) {
        if new_total_supply == self.global_ampl_supply {}

        self.internal_rebase(new_total_supply)
    }

    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, _memo: Option<String>) {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        self.internal_transfer(&sender_id, &receiver_id, amount, _memo);
    }

    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert_one_yocto();
        require!(
            env::prepaid_gas() > GAS_FOR_FT_TRANSFER_CALL,
            "More gas is required"
        );
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        self.internal_transfer(&sender_id, &receiver_id, amount, memo);
        // Initiating receiver's call and the callback
        ext_ft_receiver::ext(receiver_id.clone())
            .with_static_gas(env::prepaid_gas() - GAS_FOR_FT_TRANSFER_CALL)
            .ft_on_transfer(sender_id.clone(), amount.into(), msg)
            .then(
                ext_ft_resolver::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
                    .ft_resolve_transfer(sender_id, receiver_id, amount.into()),
            )
            .into()
    }
}

// custom fungible token core for rebase token
impl_fungible_token_core!(BridgeToken, token);
near_contract_standards::impl_fungible_token_storage!(BridgeToken, token);

#[near_bindgen]
impl FungibleTokenMetadataProvider for BridgeToken {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: self.name.clone(),
            symbol: self.symbol.clone(),
            #[cfg(feature = "migrate_icon")]
            icon: self.icon.clone(),
            #[cfg(not(feature = "migrate_icon"))]
            icon: None,
            reference: Some(self.reference.clone()),
            reference_hash: Some(self.reference_hash.clone()),
            decimals: self.decimals,
        }
    }
}

admin_controlled::impl_admin_controlled!(BridgeToken, paused);

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::{testing_env, Balance};
    // use near_sdk::MockedBlockchain;

    const GLOBAL_TOTAL_SUPPLY: Balance = 1_000_000_000;

    macro_rules! inner_set_env {
        ($builder:ident) => {
            $builder
        };

        ($builder:ident, $key:ident:$value:expr $(,$key_tail:ident:$value_tail:expr)*) => {
            {
               $builder.$key($value.try_into().unwrap());
               inner_set_env!($builder $(,$key_tail:$value_tail)*)
            }
        };
    }

    macro_rules! set_env {
        ($($key:ident:$value:expr),* $(,)?) => {
            let mut builder = VMContextBuilder::new();
            let mut builder = &mut builder;
            builder = inner_set_env!(builder, $($key: $value),*);
            testing_env!(builder.build());
        };
    }

    fn alice() -> AccountId {
        "alice.near".parse().unwrap()
    }

    fn bob() -> AccountId {
        "bob.near".parse().unwrap()
    }

    // fn get_context(predecessor_account_id: AccountId) -> VMContextBuilder {
    //     let mut builder = VMContextBuilder::new();
    //     builder
    //         .current_account_id(accounts(0))
    //         .signer_account_id(predecessor_account_id.clone())
    //         .predecessor_account_id(predecessor_account_id);
    //     builder
    // }

    #[test]
    fn test_new() {
        set_env!(predecessor_account_id: alice());
        let contract = BridgeToken::new(GLOBAL_TOTAL_SUPPLY);
        assert_eq!(contract.global_ampl_supply, GLOBAL_TOTAL_SUPPLY);
    }

    #[test]
    #[should_panic(expected = "The contract is not initialized")]
    fn test_default() {
        set_env!(predecessor_account_id: alice());
        let _contract = BridgeToken::default();
    }

    #[test]
    fn test_mint() {
        set_env!(predecessor_account_id: alice());
        let mut contract = BridgeToken::new(GLOBAL_TOTAL_SUPPLY.into());


        set_env!(predecessor_account_id: alice(), current_account_id: bob());
        contract.storage_deposit(None, None);
        let mint_amount = GLOBAL_TOTAL_SUPPLY / 3;
        contract.mint(bob(), mint_amount);

        assert_eq!(contract.ft_total_supply(), mint_amount.into());
    }

    // #[test]
    // fn test_mint() {
    //     let mut context = get_context(accounts(2));
    //     testing_env!(context.build());
    //     let mut contract = BridgeToken::new(GLOBAL_TOTAL_SUPPLY.into());
    //     testing_env!(context
    //         .storage_usage(env::storage_usage())
    //         .attached_deposit(contract.storage_balance_bounds().min.into())
    //         .predecessor_account_id(accounts(2))
    //         .build());
    //     // Paying for account registration, aka storage deposit
    //     contract.storage_deposit(None, None);

    //     testing_env!(context
    //         .storage_usage(env::storage_usage())
    //         .attached_deposit(1)
    //         .predecessor_account_id(accounts(2))
    //         .build());
    //     let mint_amount = GLOBAL_TOTAL_SUPPLY / 3;
    //     contract.mint(accounts(2), mint_amount.into());

    //     assert_eq!(contract.ft_total_supply(), mint_amount.into());

    // testing_env!(context
    //     .storage_usage(env::storage_usage())
    //     .account_balance(env::account_balance())
    //     .is_view(true)
    //     .attached_deposit(0)
    //     .build());
    // assert_eq!(contract.ft_total_supply(), mint_amount.into());
}

// #[test]
// fn test_transfer() {
//     let mut context = get_context(accounts(2));
//     testing_env!(context.build());
//     let mut contract = BridgeToken::new(GLOBAL_TOTAL_SUPPLY.into());
//     testing_env!(context
//         .storage_usage(env::storage_usage())
//         .attached_deposit(contract.storage_balance_bounds().min.into())
//         .predecessor_account_id(accounts(1))
//         .build());
//     // Paying for account registration, aka storage deposit
//     contract.storage_deposit(None, None);

//     testing_env!(context
//         .storage_usage(env::storage_usage())
//         .attached_deposit(1)
//         .predecessor_account_id(accounts(2))
//         .build());
//     let transfer_amount = GLOBAL_TOTAL_SUPPLY / 3;
//     contract.ft_transfer(accounts(1), transfer_amount.into(), None);

//     testing_env!(context
//         .storage_usage(env::storage_usage())
//         .account_balance(env::account_balance())
//         .is_view(true)
//         .attached_deposit(0)
//         .build());
//     assert_eq!(contract.ft_balance_of(accounts(2)).0, (GLOBAL_TOTAL_SUPPLY - transfer_amount));
//     assert_eq!(contract.ft_balance_of(accounts(1)).0, transfer_amount);
// }
// }

// Migration

#[cfg(feature = "migrate_icon")]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct BridgeTokenV0 {
    controller: AccountId,
    token: FungibleToken,
    name: String,
    symbol: String,
    reference: String,
    reference_hash: Base64VecU8,
    global_ampl_supply: Balance,
    gons_per_ampl: Balance,
    gons_accounts: LookupMap<AccountId, Balance>,
    gons_supply: Balance,
    decimals: u8,
    paused: Mask,
}

#[cfg(feature = "migrate_icon")]
impl From<BridgeTokenV0> for BridgeToken {
    fn from(obj: BridgeTokenV0) -> Self {
        Self {
            controller: obj.controller,
            token: obj.token,
            name: obj.name,
            symbol: obj.symbol,
            reference: obj.reference,
            reference_hash: obj.reference_hash,
            global_ampl_supply: obj.global_ampl_supply,
            gons_per_ampl: obj.gons_per_ampl,
            gons_accounts: obj.gons_accounts,
            gons_supply: obj.gons_supply,
            decimals: obj.decimals,
            paused: obj.paused,
            icon: None,
        }
    }
}

#[cfg(feature = "migrate_icon")]
#[near_bindgen]
impl BridgeToken {
    /// Adding icon as suggested here: https://nomicon.io/Standards/FungibleToken/Metadata.html
    /// This function can only be called from the factory or from the contract itself.
    #[init(ignore_state)]
    pub fn migrate_nep_148_add_icon() -> Self {
        let old_state: BridgeTokenV0 = env::state_read()
            .expect("State is not compatible with BridgeTokenV0. Migration has not been applied.");
        let new_state: BridgeToken = old_state.into();
        assert!(new_state.controller_or_self());
        new_state
    }
}

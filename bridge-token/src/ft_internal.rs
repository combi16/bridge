use crate::*;

impl BridgeToken {
    pub fn internal_unwrap_balance_of(&mut self, account_id: &AccountId) -> (Balance, Balance) {
        self.internal_update_scaler();
        if let Some(gons_balance) = self.gons_accounts.get(account_id) {
            match gons_balance.checked_div(self.gons_per_ampl) {
                Some(ampl_balance) => (ampl_balance, gons_balance),
                _ => env::panic_str(format!("Gons balance overflow").as_str()),
            }
        } else {
            env::panic_str(format!("The account {} is not registered", &account_id).as_str());
        }
    }

    pub fn internal_deposit(&mut self, account_id: &AccountId, amount: Balance) {
        let (gon_balance, ampl_balance) = self.internal_unwrap_balance_of(account_id);

        if let Some(new_balance) = gon_balance.checked_add(amount) {
            self.gons_accounts.insert(account_id, &new_balance);
            self.gons_supply = self
                .gons_supply
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("Gons supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }

        if let Some(new_balance) = ampl_balance.checked_add(amount) {
            self.token.accounts.insert(account_id, &new_balance);
            self.token.total_supply = self
                .token
                .total_supply
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("Total supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }
    }

    pub fn internal_withdraw(&mut self, account_id: &AccountId, amount: Balance) {
        let (gon_balance, ampl_balance) = self.internal_unwrap_balance_of(account_id);
        //withdraw gons
        if let Some(new_balance) = gon_balance.checked_sub(amount) {
            self.gons_accounts.insert(account_id, &new_balance);
            self.gons_supply = self
                .gons_supply
                .checked_sub(amount)
                .unwrap_or_else(|| env::panic_str("Gons supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }
        // withdraw wrapped-ampl
        if let Some(new_balance) = ampl_balance.checked_sub(amount) {
            self.token.accounts.insert(account_id, &new_balance);
            self.token.total_supply = self
                .token
                .total_supply
                .checked_sub(amount)
                .unwrap_or_else(|| env::panic_str("Total supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }
    }

    pub fn internal_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        amount: Balance,
        memo: Option<String>,
    ) {
        require!(
            sender_id != receiver_id,
            "Sender and receiver should be different"
        );
        require!(amount > 0, "The amount should be a positive number");
        self.internal_withdraw(sender_id, amount);
        self.internal_deposit(receiver_id, amount);
        FtTransfer {
            old_owner_id: sender_id,
            new_owner_id: receiver_id,
            amount: &U128(amount),
            memo: memo.as_deref(),
        }
        .emit();
    }

    pub fn internal_update_scaler(&mut self) {
        let current_global_ampl_supply = self.global_ampl_supply;

        if let Some(current_gons_per_ampl) = TOTAL_GONS.checked_div(current_global_ampl_supply) {
            self.gons_per_ampl = current_gons_per_ampl;
        } else {
            env::panic_str("Total gons overflow");
        }
    }

    pub fn internal_update_token_supply(&mut self, new_total_supply: Balance) {
        let prev_global_ampl_supply = self.global_ampl_supply;

        if let Some(temp_global_total_supply) = self
            .token
            .total_supply
            .checked_mul(new_total_supply.clone())
        {
            match temp_global_total_supply.checked_div(prev_global_ampl_supply) {
                Some(new_wampl_total_supply) => {
                    self.token.total_supply = new_wampl_total_supply;
                    self.global_ampl_supply = new_total_supply;
                }
                None => {
                    env::panic_str(format!("Total supply overflow",).as_str());
                }
            }
        }
    }

    pub fn internal_rebase(
        &mut self,
        new_total_supply: Balance,
    ) {
        // update w-ample total supply
        self.internal_update_token_supply(new_total_supply);
        
        // update scaler
        self.internal_update_scaler();
    }

    pub fn internal_register_account(&mut self, account_id: &AccountId) {
        if self.token.accounts.insert(account_id, &0).is_some() {
            env::panic_str("The account is already registered");
        }
        if self.gons_accounts.insert(account_id, &0).is_some() {
            env::panic_str("The account is already registered");
        }
    }
}

use crate::*;

impl BridgeToken {
    pub fn internal_unwrap_balance_of(&self, account_id: &AccountId) -> (Balance, Balance) {
        if let Some(gons_balance) = self.gons_accounts.get(account_id) {
            match self.token.accounts.get(account_id) {
                Some(ampl_balance) => return (gons_balance, ampl_balance),
                None => env::panic_str(
                    format!("The account {} is not registered", &account_id).as_str(),
                ),
            }
        } else {
            env::panic_str(format!("The account {} is not registered", &account_id).as_str());
        }
    }

    pub fn internal_deposit(&mut self, account_id: &AccountId, amount: Balance) {
        let (gon_balance, ampl_balance) = self.internal_unwrap_balance_of(account_id);
        //update gons
        if let Some(new_balance) = gon_balance.checked_add(amount) {
            self.gons_accounts.insert(account_id, &new_balance);
            self.gons_supply = self
                .gons_supply
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("Gons supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }
        // update wrapped-ampl
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
        //update gons
        if let Some(new_balance) = gon_balance.checked_sub(amount) {
            self.gons_accounts.insert(account_id, &new_balance);
            self.gons_supply = self
                .gons_supply
                .checked_sub(amount)
                .unwrap_or_else(|| env::panic_str("Gons supply overflow"));
        } else {
            env::panic_str("Balance overflow");
        }
        // update wrapped-ampl
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

    pub fn internal_rebase(
        &mut self,
        _epoch: U128,
        new_global_total_supply: Balance,
        prev_global_ampl_supply: Balance,
    ) {
        // update w-ample total supply
        if let Some(_supply) = self
            .token
            .total_supply
            .checked_mul(new_global_total_supply.clone())
        {
            match _supply.checked_div(prev_global_ampl_supply) {
                Some(new_wampl_total_supply) => {
                    self.token.total_supply = new_wampl_total_supply;
                }
                None => {}
            }
            // update scaler
            match TOTAL_GONS.checked_div(self.global_ampl_supply) {
                Some(new_gons_per_ampl) => {
                    self.gons_per_ampl = new_gons_per_ampl;
                }
                None => {}
            }
        } else {
            env::panic_str(format!("The token is not registered",).as_str());
        }
    }
}

use bridge_common::prover::{EthAddress, EthEventParams, EthRebaseEvent};
use ethabi::{ParamType, Token};
use hex::ToHex;
use near_sdk::{Balance};

/// Data that was emitted by the Ethereum Rebase event.
#[derive(Debug, Eq, PartialEq)]
pub struct EthRebasedEvent {
    pub rebaser_address: EthAddress,
    pub token: String,
    pub epoch: u128,
    pub total_supply: Balance,
}

impl EthRebasedEvent {
    fn event_params() -> EthEventParams {
        vec![
            ("token".to_string(), ParamType::Address, true),
            ("epoch".to_string(), ParamType::Uint(256), true),
            ("total_supply".to_string(), ParamType::Uint(256), false),
        ]
    }

    /// Parse raw log entry data.
    pub fn from_log_entry_data(data: &[u8]) -> Self {
        let event =
            EthRebaseEvent::from_log_entry_data("LogRebase", EthRebasedEvent::event_params(), data);
        let token = event.log.params[0].value.clone().to_address().unwrap().0;
        let token = (&token).encode_hex::<String>();
        let epoch = event.log.params[1]
            .value
            .clone()
            .to_uint()
            .unwrap()
            .as_u128();
        let total_supply = event.log.params[2]
            .value
            .clone()
            .to_uint()
            .unwrap()
            .as_u128();
        Self {
            rebaser_address: event.rebaser_address,
            token,
            epoch,
            total_supply,
        }
    }
    
    pub fn to_log_entry_data(&self) -> Vec<u8> {
        EthRebaseEvent::to_log_entry_data(
            "LogRebase",
            EthRebasedEvent::event_params(),
            self.rebaser_address,
            vec![
                hex::decode(self.token.clone()).unwrap(),
                self.rebaser_address.to_vec(),
            ],
            vec![
                Token::Uint(self.epoch.into()),
                Token::Uint(self.total_supply.into()),
            ],
        )
    }
}

impl std::fmt::Display for EthRebasedEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "token: {}; epoch: {}; total_supply: {}",
            self.token, self.epoch, self.total_supply
        )
    }
}

#[cfg(test)]
mod tests {
    use super::EthRebasedEvent;
    use rand::prelude::ThreadRng;
    use rand::Rng;

    #[test]
    fn test_event_data() {
        let event_data = EthRebasedEvent {
            rebaser_address: rng.gen::<[u8; 20]>(),
            token: hex::encode(rng.gen::<[u8; 20]>()),
            epoch: rng.gen::<u128>(),
            total_supply: rng.gen::<u128>(),
        };
        let data = event_data.to_log_entry_data();
        let result = EthRebasedEvent::from_log_entry_data(&data);
        assert_eq!(result, event_data);
    }
}

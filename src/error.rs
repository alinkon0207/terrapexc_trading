use cosmwasm_std::{OverflowError, StdError};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Not Reward or Order token")]
    UnacceptableToken {},

    #[error("Invalid zero amount")]
    InvalidZeroAmount {},

    #[error("InvalidInput")]
    InvalidInput {},

    #[error("Not enough Reward")]
    NotEnoughReward {},

    #[error("Not reward token")]
    NotRewardToken {},

    #[error("Asset mismatch")]
    AssetMismatch {},

    #[error("Too small offer amount")]
    TooSmallOfferAmount {},

    #[error("Still in Lock period")]
    StillInLock {},

    #[error("Not unstakable token")]
    NotUnstakeToken {},

    #[error("Not match order")]
    NotMatchOrder {},

    #[error("Not find order")]
    NotFindOrder {},

    #[error("Disabled")]
    Disabled {},

    #[error("Already started order trading, please try 10 seconds later.")]
    AlreadyStarted {},

    #[error("Not started order trading")]
    NotStarted {},

    #[error("Not Same Price")]
    NotSamePrice {},
}

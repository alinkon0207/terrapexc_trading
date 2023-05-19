use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use classic_terrapexc::trading::{PairInfo, TraderRecord};
use cosmwasm_std::CanonicalAddr;
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: CanonicalAddr,
    pub pair_list: Vec<PairInfo>,
    pub enabled: bool,
}

pub const CONFIG_KEY: &str = "config";
pub const CONFIG: Item<Config> = Item::new("config");

pub const BUYERS: Map<String, TraderRecord> = Map::new("buyers");
pub const SELLERS: Map<String, TraderRecord> = Map::new("sellers");

use crate::error::ContractError;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, from_binary, to_binary, Addr, Api, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Order, QuerierWrapper, Response, StdResult, Storage, Uint128,
};
use cw2::set_contract_version;
use cw20::{Cw20ReceiveMsg, Denom};
use cw_storage_plus::Bound;

use crate::state::{Config, BUYERS, CONFIG, SELLERS};
use crate::util;

use classic_terrapexc::asset::AssetInfo;
use classic_terrapexc::querier::{query_balance, query_token_balance};
use classic_terrapexc::trading::{
    ConfigResponse, ExecuteMsg, InstantiateMsg, MatchOrderResponse, MigrateMsg, PairInfo, QueryMsg,
    ReceiveMsg, TraderInfo, TraderListResponse, TraderRecord,
};

pub const NORMAL_DECIMAL: u128 = 1000000u128;
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:terrapexc-trading";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        owner: deps.api.addr_canonicalize(info.sender.as_str())?,
        pair_list: msg.pair_list,
        enabled: msg.enabled,
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            pair_list,
            enabled,
        } => execute_update_config(deps, env, info, owner, pair_list, enabled),
        ExecuteMsg::Receive(msg) => execute_receive(deps, msg),
        ExecuteMsg::Order {
            order,
            add_order,
            update_order,
            remove_orders
        } => execute_order(deps, order, add_order, update_order, remove_orders),
        ExecuteMsg::Cancel { order_id, is_buy } => execute_cancel(deps, info, order_id, is_buy),
    }
}

//////////////////////////////////////////////////
// Description:  Only owner can execute it
// Params: [1] - Owner
//         [2] - Trading Pair List
//         [3] - Enabled
/////////////////////////////////////////////////
pub fn execute_update_config(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    owner: Option<String>,
    pair_list: Option<Vec<PairInfo>>,
    enabled: Option<bool>,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;

    check_owner(&deps, &info)?;

    if let Some(owner) = owner {
        // validate address format
        let _ = deps.api.addr_validate(&owner)?;

        config.owner = deps.api.addr_canonicalize(&owner)?;
    }

    if let Some(pair_list) = pair_list {
        config.pair_list = pair_list;
    }

    if let Some(enabled) = enabled {
        config.enabled = enabled;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attribute("action", "update_config"))
}

///////////////////////////////////////////////////////////
//   Description: receive messages
//   Params: [1] - wrapper - Cw20ReceiveMsg
///////////////////////////////////////////////////////////
pub fn execute_receive(
    deps: DepsMut,
    wrapper: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    if wrapper.amount == Uint128::zero() {
        return Err(ContractError::InvalidInput {});
    }
    // let config: Config = CONFIG.load(deps.storage)?;

    // let user_addr = &deps.api.addr_validate(&wrapper.sender)?;
    // let msg: ReceiveMsg = from_binary(&wrapper.msg)?;
    return Ok(Response::new());
}

pub fn execute_order(
    deps: DepsMut,
    order: TraderRecord,
    add_order: Option<TraderRecord>,
    update_order: Option<TraderRecord>,
    remove_orders: Option<Vec<TraderRecord>>,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let pair_info = cfg.pair_list.get(order.pair_id.u128() as usize).unwrap();

    if order.current_stock_amount == Uint128::zero() {
        return Err(ContractError::InvalidZeroAmount {});
    }
    // check out if buyer's balance is enough.
    if order.is_buy {
        let remains = match pair_info.from_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&deps.querier, order.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &deps.querier,
                deps.api.addr_validate(contract_addr.as_str())?,
                order.address.clone(),
            )?,
        };
        let other_remains = remains * order.price / Uint128::from(NORMAL_DECIMAL);
        if other_remains < order.current_stock_amount {
            return Err(ContractError::InvalidInput {});
        }
    } else {
        let remains = match pair_info.to_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&deps.querier, order.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &deps.querier,
                deps.api.addr_validate(contract_addr.as_str())?,
                order.address.clone(),
            )?,
        };
        if remains < order.current_stock_amount {
            return Err(ContractError::InvalidInput {});
        }
    }

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut match_orders: Vec<MatchOrderResponse> = vec![];

    if let Some(add_order) = add_order {
        add_orderbook(deps.storage, add_order)?;
    }

    if let Some(update_order) = update_order {
        match_orders.push(update_orderbook(deps.storage, deps.api, deps.querier, &order, &update_order)?);
    }

    if let Some(remove_orders) = remove_orders {
        for remove_order in remove_orders.iter() {
            match_orders.push(remove_orderbook(deps.storage, deps.api, deps.querier, &order, remove_order)?);
        }
    }

    for match_order in match_orders.iter() {
        let MatchOrderResponse {
            buyer,
            seller,
            move_amount,
        } = match_order;
        
        let other_move_amount =
            move_amount * order.price / Uint128::from(NORMAL_DECIMAL);
        if let AssetInfo::Token { contract_addr, .. } = &pair_info.to_asset {
            let to_address = Addr::unchecked(contract_addr);
            messages.push(util::transfer_from_token_message(
                seller.clone(),
                Denom::Cw20(to_address.clone()),
                *move_amount,
                buyer.clone(),
            )?);
        }
        if let AssetInfo::Token { contract_addr, .. } = &pair_info.from_asset {
            let from_address = Addr::unchecked(contract_addr);
            messages.push(util::transfer_from_token_message(
                buyer.clone(),
                Denom::Cw20(from_address.clone()),
                other_move_amount,
                seller.clone(),
            )?);
        }
    }

    return Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "order"),
        attr("address", order.address.clone()),
    ]));
}

pub fn execute_cancel(
    deps: DepsMut,
    info: MessageInfo,
    order_key: String,
    is_buy: bool,
) -> Result<Response, ContractError> {
    if is_buy && !BUYERS.has(deps.storage, order_key.clone())
        || !is_buy && !SELLERS.has(deps.storage, order_key.clone())
    {
        return Err(ContractError::NotStarted {});
    }
    let record: TraderRecord;
    if is_buy {
        record = BUYERS.load(deps.storage, order_key.clone())?;

        if record.address != info.sender.clone() {
            return Err(ContractError::Unauthorized {});
        }
        BUYERS.remove(deps.storage, order_key.clone());
    } else {
        record = SELLERS.load(deps.storage, order_key.clone())?;

        if record.address != info.sender.clone() {
            return Err(ContractError::Unauthorized {});
        }
        SELLERS.remove(deps.storage, order_key.clone());
    }

    return Ok(Response::new().add_attributes(vec![attr("action", "cancel")]));

    // should cancel approve
}

pub fn add_orderbook(
    storage: &mut dyn Storage,
    order: TraderRecord,
) -> Result<bool, ContractError> {
    let key = order.id.clone();

    if order.is_buy {
        if BUYERS.has(storage, key.clone()) {
            return Err(ContractError::AlreadyStarted {});
        }

        BUYERS.save(storage, key.clone(), &order)?;
        return Ok(true);
    } else {
        if SELLERS.has(storage, key.clone()) {
            return Err(ContractError::AlreadyStarted {});
        }

        SELLERS.save(storage, key.clone(), &order)?;
        return Ok(true);
    }
}

pub fn update_orderbook(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    order: &TraderRecord,
    update_order: &TraderRecord,
) -> Result<MatchOrderResponse, ContractError> {
    let cfg = CONFIG.load(storage)?;
    let pair_info = cfg.pair_list.get(update_order.pair_id.u128() as usize).unwrap();
    let key = update_order.id.clone();
    let move_amount = update_order.current_stock_amount;

    if update_order.is_buy {
        if !BUYERS.has(storage, key.clone()) {
            return Err(ContractError::NotStarted {});
        }
        let mut buyer_record = BUYERS.load(storage, key.clone())?;
        
        // check out if update is possible
        if move_amount >= buyer_record.current_stock_amount {
            return Err(ContractError::NotStarted {});
        }

        // check out if buyer's balance is enough.
        let remains = match pair_info.from_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&querier, buyer_record.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &querier,
                api.addr_validate(contract_addr.as_str())?,
                buyer_record.address.clone(),
            )?,
        };
        let other_remains = remains / update_order.price * Uint128::from(NORMAL_DECIMAL);
        if other_remains < move_amount {
            return Err(ContractError::NotEnoughReward {});
        }

        buyer_record.current_stock_amount -= move_amount;
        BUYERS.save(storage, key.clone(), &buyer_record)?;

        return Ok(MatchOrderResponse {
            buyer: buyer_record.address.clone(),
            seller: order.address.clone(),
            move_amount: move_amount,
        });
    } else {
        if !SELLERS.has(storage, key.clone()) {
            return Err(ContractError::NotStarted {});
        }
        let mut seller_record = SELLERS.load(storage, key.clone())?;

        // check out if update is possible
        if move_amount >= seller_record.current_stock_amount {
            return Err(ContractError::NotStarted {});
        }

        // check out if seller's balance is enough.
        let remains = match pair_info.to_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&querier, seller_record.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &querier,
                api.addr_validate(contract_addr.as_str())?,
                seller_record.address.clone(),
            )?,
        };
        if remains < move_amount {
            return Err(ContractError::NotEnoughReward {});
        }

        seller_record.current_stock_amount -= move_amount;
        SELLERS.save(storage, key.clone(), &seller_record)?;

        return Ok(MatchOrderResponse {
            buyer: order.address.clone(),
            seller: seller_record.address.clone(),
            move_amount: move_amount,
        });
    }
}

pub fn remove_orderbook(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    order: &TraderRecord,
    remove_order: &TraderRecord,
) -> Result<MatchOrderResponse, ContractError> {
    let cfg = CONFIG.load(storage)?;
    let pair_info = cfg.pair_list.get(remove_order.pair_id.u128() as usize).unwrap();
    let key = remove_order.id.clone();
    let move_amount = remove_order.current_stock_amount;

    if remove_order.is_buy {
        if !BUYERS.has(storage, key.clone()) {
            return Err(ContractError::NotStarted {});
        }
        let buyer_record = BUYERS.load(storage, key.clone())?;

        // check out if buyer's balance is enough.
        let remains = match pair_info.from_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&querier, buyer_record.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &querier,
                api.addr_validate(contract_addr.as_str())?,
                buyer_record.address.clone(),
            )?,
        };
        let other_remains = remains / remove_order.price * Uint128::from(NORMAL_DECIMAL);
        if other_remains < move_amount {
            return Err(ContractError::NotEnoughReward {});
        }

        BUYERS.remove(storage, key.clone());

        return Ok(MatchOrderResponse {
            buyer: buyer_record.address.clone(),
            seller: order.address.clone(),
            move_amount: move_amount,
        });
    } else {
        if !SELLERS.has(storage, key.clone()) {
            return Err(ContractError::NotStarted {});
        }
        let seller_record = SELLERS.load(storage, key.clone())?;

        // check out if seller's balance is enough.
        let remains = match pair_info.to_asset.clone() {
            AssetInfo::NativeToken { denom } => {
                query_balance(&querier, seller_record.address.clone(), denom)?
            }
            AssetInfo::Token { contract_addr } => query_token_balance(
                &querier,
                api.addr_validate(contract_addr.as_str())?,
                seller_record.address.clone(),
            )?,
        };
        if remains < move_amount {
            return Err(ContractError::NotEnoughReward {});
        }

        SELLERS.remove(storage, key.clone());

        return Ok(MatchOrderResponse {
            buyer: order.address.clone(),
            seller: seller_record.address.clone(),
            move_amount: move_amount,
        });
    }
}

pub fn check_owner(deps: &DepsMut, info: &MessageInfo) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;

    if deps.api.addr_canonicalize(info.sender.as_str())? != cfg.owner {
        return Err(ContractError::Unauthorized {});
    }

    Ok(Response::new().add_attribute("action", "check_owner"))
}

pub fn check_enabled(deps: &DepsMut, _info: &MessageInfo) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if !config.enabled {
        return Err(ContractError::Disabled {});
    }
    Ok(Response::new().add_attribute("action", "check_enabled"))
}

//----------------------------------------------------------------------//
//                                QUERY                                 //
//----------------------------------------------------------------------//
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::Now {} => to_binary(&query_get_now(_env)?),
        QueryMsg::ListOrders {
            is_buy,
            start_after,
            limit,
        } => to_binary(&query_list_traders(deps, is_buy, start_after, limit)?),
    }
}

pub fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    Ok(ConfigResponse {
        owner: deps.api.addr_humanize(&cfg.owner)?.to_string(),
        pair_list: cfg.pair_list,
        enabled: cfg.enabled,
    })
}

pub fn query_get_now(env: Env) -> StdResult<u64> {
    Ok(env.block.time.seconds())
}

fn map_trader(item: StdResult<(Vec<u8>, TraderRecord)>) -> StdResult<TraderInfo> {
    item.map(|(id, record)| TraderInfo {
        id: String::from_utf8(id).unwrap(),
        address: record.address,
        order_stock_amount: record.order_stock_amount,
        current_stock_amount: record.current_stock_amount,
        price: record.price,
    })
}

// settings for pagination
const MAX_LIMIT: u32 = 30;
const DEFAULT_LIMIT: u32 = 10;
fn query_list_traders(
    deps: Deps,
    is_buy: bool,
    start_after: Option<String>,
    limit: Option<u32>,
) -> StdResult<TraderListResponse> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;

    let start = start_after.map(|id| Bound::exclusive(id));

    let stakers;
    if is_buy {
        stakers = BUYERS
            .range(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .map(|item| map_trader(item))
            .collect::<StdResult<Vec<_>>>()?;
    } else {
        stakers = SELLERS
            .range(deps.storage, start, None, Order::Ascending)
            .take(limit)
            .map(|item| map_trader(item))
            .collect::<StdResult<Vec<_>>>()?;
    }

    Ok(TraderListResponse { traders: stakers })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::default())
}

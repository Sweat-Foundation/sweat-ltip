use near_sdk::{json_types::U128, near, AccountId};
use near_sdk_contract_tools::Nep297;

#[derive(Nep297)]
#[near(serializers = [json])]
#[nep297(standard = "nep171", version = "0.1.0", rename_all = "snake_case")]
pub enum LtipEvent {
    OrderUpdate(Vec<OrderUpdateData>),
    Terminate((AccountId, Vec<(u32, u128)>)),
}

#[near(serializers = [json])]
pub struct OrderUpdateData {
    pub issue_at: u32,
    pub amount: U128,
}

use crate::ids::AccountId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub uuid: AccountId,
    pub username: String,
    pub added_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSummary {
    pub uuid: AccountId,
    pub username: String,
    pub selected: bool,
}

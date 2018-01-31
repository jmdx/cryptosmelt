use schema::*;
use diesel::sql_types::*;
use chrono::NaiveDateTime;

#[derive(Queryable)]
pub struct BlockProgress {
  pub id: i32,
  pub created: NaiveDateTime,
  pub block_depth: i64,
  pub block_id: String,
}
#[derive(Insertable)]
#[table_name="block_progress"]
pub struct NewBlockProgress<'a> {
  pub block_depth: i64,
  pub block_id: &'a str,
}

pub enum BlockStatus {
  Submitted, Orphaned, Unlocked
}
impl Into<i32> for BlockStatus {
  fn into(self) -> i32 {
    match self {
      BlockStatus::Submitted => 0,
      BlockStatus::Orphaned => 1,
      BlockStatus::Unlocked => 2,
    }
  }
}
impl From<i32> for BlockStatus {
  fn from(i: i32) -> BlockStatus {
    match i {
      0 => BlockStatus::Submitted,
      1 => BlockStatus::Orphaned,
      _ => BlockStatus::Unlocked,
    }
  }
}
#[derive(Queryable)]
pub struct FoundBlock {
  pub block_id: String,
  pub created: NaiveDateTime,
  pub height: i64,
  pub status: i32,
}
#[derive(Insertable)]
#[table_name="found_block"]
pub struct NewFoundBlock<'a> {
  pub block_id: &'a str,
  pub height: i64,
  pub status: i32,
}

#[derive(Queryable)]
pub struct MinerBalance {
  pub id: i32,
  pub created: NaiveDateTime,
  pub address: String,
  pub change: i64,
  pub payment_transaction: Option<String>,
  pub is_fee: bool,
}
#[derive(Insertable)]
#[table_name="miner_balance"]
pub struct NewMinerBalance<'a> {
  pub address: &'a str,
  pub change: i64,
  pub payment_transaction: Option<&'a str>,
  pub is_fee: bool,
}

#[derive(Queryable)]
pub struct PoolPayment {
  pub id: i32,
  pub created: NaiveDateTime,
  pub payment_transaction: String,
  pub fee: i64,
}
#[derive(Insertable)]
#[table_name="pool_payment"]
pub struct NewPoolPayment<'a> {
  pub payment_transaction: &'a str,
  pub fee: i64,
}

#[derive(Queryable)]
pub struct ValidShare {
  pub id: i32,
  pub created: NaiveDateTime,
  pub address: String,
  pub miner_alias: String,
  pub shares: i64,
}
#[derive(Insertable)]
#[table_name="valid_share"]
pub struct NewShare<'a> {
  pub address: &'a str,
  pub miner_alias: &'a str,
  pub shares: i64,
}

#[derive(QueryableByName, Serialize)]
pub struct MinerStats {
  #[sql_type="Int8"]
  #[column_name="shares"]
  pub shares: i64,

  #[sql_type="Varchar"]
  #[column_name="miner_alias"]
  pub miner_alias: String,

  #[sql_type="Timestamp"]
  #[column_name="created_minute"]
  pub created_minute: NaiveDateTime,
}

#[derive(QueryableByName, Serialize, Debug)]
pub struct ShareTotal {
  #[sql_type="Int8"]
  #[column_name="shares"]
  pub shares: i64,

  #[sql_type="Varchar"]
  #[column_name="address"]
  pub address: String,
}

#[derive(QueryableByName, Serialize, Debug)]
pub struct MinerBalanceTotal {
  #[sql_type="Int8"]
  #[column_name="amount"]
  pub amount: i64,

  #[sql_type="Varchar"]
  #[column_name="address"]
  pub address: String,
}
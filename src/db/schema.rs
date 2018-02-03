table! {
    block_progress (id) {
        id -> Int4,
        created -> Timestamp,
        block_depth -> Int8,
        block_id -> Text,
    }
}

table! {
    found_block (block_id) {
        block_id -> Text,
        created -> Timestamp,
        height -> Int8,
        status -> Int4,
    }
}

table! {
    miner_balance (id) {
        id -> Int4,
        created -> Timestamp,
        address -> Varchar,
        change -> Int8,
        payment_transaction -> Nullable<Text>,
        is_fee -> Bool,
    }
}

table! {
    pool_payment (id) {
        id -> Int4,
        created -> Timestamp,
        payment_transaction -> Text,
        fee -> Int8,
    }
}

table! {
    valid_share (id) {
        id -> Int4,
        created -> Timestamp,
        address -> Varchar,
        miner_alias -> Varchar,
        shares -> Int8,
    }
}

joinable!(block_progress -> found_block (block_id));

allow_tables_to_appear_in_same_query!(
    block_progress,
    found_block,
    miner_balance,
    pool_payment,
    valid_share,
);

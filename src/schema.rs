table! {
    crypto_rate (date, coin) {
        date -> Timestamp,
        coin -> Text,
        rate -> Float,
    }
}

table! {
    reminders (id) {
        id -> Nullable<Integer>,
        target_chan -> Nullable<Text>,
        nick -> Text,
        created_at -> Text,
        remind_at -> Text,
        content -> Text,
    }
}

table! {
    user_settings (nick) {
        nick -> Nullable<Text>,
        timezone -> Nullable<Text>,
    }
}

allow_tables_to_appear_in_same_query!(
    crypto_rate,
    reminders,
    user_settings,
);

use twitch_api2::eventsub::stream::{StreamOfflineV1Payload, StreamOnlineV1Payload};

#[derive(Debug)]
pub enum Message {
    StreamOnline(StreamOnlineV1Payload),
    StreamOffline(StreamOfflineV1Payload),
}

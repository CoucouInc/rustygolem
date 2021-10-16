use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use twitch_api2::{helix::streams::Stream, types::Nickname};

#[derive(Debug, Default)]
pub struct State {
    // indices corresponding to Config.watched_streams
    // to identify which watched streams are currently online.
    online_streams: Arc<Mutex<HashMap<Nickname, Stream>>>,
}

impl State {
    pub fn add_stream(&self, nick: Nickname, stream: Stream) {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .insert(nick, stream);
    }

    pub fn remove_stream(&self, nick: &Nickname) -> Option<Stream> {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .remove(nick)
    }
}

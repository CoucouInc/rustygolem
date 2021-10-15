use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

#[derive(Debug, Default)]
pub struct State {
    // indices corresponding to Config.watched_streams
    // to identify which watched streams are currently online.
    online_streams: Arc<Mutex<BTreeSet<usize>>>,
}

impl State {
    pub fn add_stream(&self, idx: usize) {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .insert(idx);
    }

    pub fn remove_stream(&self, idx: usize) -> bool {
        self.online_streams
            .lock()
            .expect("twitch state lock")
            .remove(&idx)
    }
}

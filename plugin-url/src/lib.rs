use async_trait::async_trait;
use plugin_core::{Plugin, Result};

pub struct Url {}

#[async_trait]
impl Plugin for Url {
    async fn init() -> Result<Self> {
        todo!()
    }

    fn get_name(&self) -> &'static str {
        "url"
    }
}

use super::HlEngineApi;
use reth::{
    api::{AddOnsContext, FullNodeComponents},
    builder::rpc::EngineApiBuilder,
};

/// Builder for mocked [`HlEngineApi`] implementation.
#[derive(Debug, Default)]
pub struct HlEngineApiBuilder;

impl<N> EngineApiBuilder<N> for HlEngineApiBuilder
where
    N: FullNodeComponents,
{
    type EngineApi = HlEngineApi;

    async fn build_engine_api(self, _ctx: &AddOnsContext<'_, N>) -> eyre::Result<Self::EngineApi> {
        Ok(HlEngineApi::default())
    }
}

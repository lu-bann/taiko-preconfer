mod preconf_whitelist;
pub mod taiko_inbox;
use alloy_provider::{
    Identity, RootProvider,
    fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
pub use preconf_whitelist::PreconfWhitelist;
pub use taiko_inbox::{TaikoAnchor, TaikoInbox};

pub type Provider = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;
pub type TaikoAnchorInstance = TaikoAnchor::TaikoAnchorInstance<Provider>;
pub type TaikoInboxInstance = TaikoInbox::TaikoInboxInstance<Provider>;
pub type TaikoWhitelistInstance = PreconfWhitelist::PreconfWhitelistInstance<Provider>;

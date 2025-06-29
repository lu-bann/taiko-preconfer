mod preconf_whitelist;
mod taiko_anchor;
pub mod taiko_inbox;
use alloy_provider::{
    Identity, RootProvider,
    fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
pub use preconf_whitelist::PreconfWhitelist;
pub use taiko_anchor::TaikoAnchor;
pub use taiko_inbox::TaikoInbox;

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

mod preconf_whitelist;
mod taiko_anchor;
mod taiko_inbox;
pub use preconf_whitelist::PreconfWhitelist;
pub mod taiko_wrapper;
use alloy_provider::{
    Identity, RootProvider,
    fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
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

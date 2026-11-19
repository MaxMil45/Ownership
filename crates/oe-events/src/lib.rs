//! Ownership event rules.
//!
//! Each rule has a stable, versioned ID (for example `insider.cluster_buy@1`)
//! and a written spec in `events/specs/`. Changing a rule's behaviour means a
//! new version, so old alerts and studies stay reproducible.

pub mod activist;
pub mod cluster_buy;
pub mod plan_terminated;
pub mod planned_sale;
pub mod trades;

pub use activist::{
    detect_activist_events, new_13d_event, passive_to_active_event, ActivistEvent, ActivistParams,
};
pub use cluster_buy::{detect_cluster_buys, ClusterBuyEvent, ClusterBuyParams};
pub use plan_terminated::{detect_plan_terminations, PlanTerminatedEvent};
pub use planned_sale::{
    detect_planned_sales, planned_sale_event, PlannedSaleEvent, PlannedSaleParams,
};
pub use trades::{trades_from_document, InsiderTrade};

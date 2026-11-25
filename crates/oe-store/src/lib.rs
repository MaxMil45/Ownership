//! Loads parsed ownership filings into the PostgreSQL point-in-time ledger
//! (`migrations/`).
//!
//! * [`rows`]: filing XML + sidecar → table rows. Pure, tested offline.
//! * [`amend`]: may an amendment's rows replace the report it amends? Pure,
//!   tested offline (ADR 0005).
//! * `sha256_hex` (from `oe-core`): fingerprint of each raw filing.
//! * [`db`]: connect, migrate, load in batches, link amendments, status.
//!   Needs a PostgreSQL server.
//!
//! This is the only crate in the workspace that depends on a database driver,
//! which keeps the core crates dependency-free (ADR 0001, ADR 0004).

pub mod amend;
pub mod company;
pub mod db;
pub mod events;
pub mod house;
pub mod officials;
pub mod other;
pub mod people;
pub mod prices;
pub mod rows;

pub use amend::{compare, RowShape, Verdict};
pub use company::{
    company, find_companies, trade_marks, Company, CompanyInsider, CompanyTrade, TradeMark,
    TradesOf,
};
pub use db::{
    check_schema, connect, flow_by_day, link_amendments, link_pass, load_batch, load_batch_with,
    maintain, migrate, status, trade_history, FlowDay, HistoryTrade, LinkReport, LoadReport,
    MaintainReport, Status, StoreError, MIGRATIONS,
};
pub use events::{
    event_counts, latest_tickers, ledger_purchases, query_events, replace_event_feed, EventFilter,
    EventPage, EventRow, LedgerPurchase, PurchasesLeftOut,
};
pub use house::{
    house_reports_to_read, member_activity, official_trades, replace_house_transactions,
    HouseLoadReport, HouseRead, HouseTxRow, MemberActivity, OfficialTrade, OfficialTradeFilter,
};
pub use oe_core::sha256_hex;
pub use officials::{
    find_officials, load_house_reports, load_members, official_by_id, HouseReportRow,
    HouseReportsReport, MembersReport, OfficialSummary,
};
pub use other::{
    adoption_history, contest_history, form144_history, load_other, prepare_other,
    schedule13g_history, ContestHeader, OtherDoc, OtherFiling, OtherReport, Stored13G,
    StoredAdoption, StoredContest, StoredNotice,
};
pub use people::{
    build_people_index, lookup, ContestFiling, Holding, InsiderRole, Lookup, PeopleIndexReport,
    PersonCard, Receipt, Trade,
};
pub use prices::{
    build_outcomes, check_series, import_prices, insider_records, outcome_summary, outcomes,
    price_status, prices_for, wanted_symbols, CheckReport, HorizonStats, ImportReport,
    InsiderRecord, OutcomeReport, OutcomeRow, OutcomeSummary, OutcomesOf, PriceSeries, PriceStatus,
    BENCHMARK, HORIZONS,
};
pub use rows::{
    prepare, prepare_bulk, FilingRow, OwnerRole, PrepareError, Prepared, Sidecar, TransactionRow,
};

/// The database connection type, re-exported so callers need no driver dependency.
pub use postgres::Client;

pub mod account;
pub mod dashboard;
pub mod diagnostics;
pub mod logs;
pub mod profiles;

pub use account::AccountView;
pub use dashboard::{DashboardAction, DashboardView};
pub use diagnostics::DiagnosticsView;
pub use logs::LogsView;
pub use profiles::ProfilesView;

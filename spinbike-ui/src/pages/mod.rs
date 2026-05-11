pub mod admin;
pub mod dashboard;
pub mod door;
pub mod login;
pub mod my_balance;
pub mod my_bookings;
pub mod reports;
pub mod schedule;
pub mod staff_dashboard;

pub use admin::AdminPage as SettingsPage;
pub use door::DoorPage;
pub use reports::ReportsPage;

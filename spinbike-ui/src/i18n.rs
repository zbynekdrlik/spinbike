use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Sk,
    En,
}

const LANG_KEY: &str = "spinbike_lang";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn get_saved_lang() -> Lang {
    storage()
        .and_then(|s| s.get_item(LANG_KEY).ok()?)
        .map(|v| match v.as_str() {
            "en" => Lang::En,
            _ => Lang::Sk,
        })
        .unwrap_or(Lang::Sk)
}

pub fn save_lang(lang: Lang) {
    if let Some(s) = storage() {
        let val = match lang {
            Lang::Sk => "sk",
            Lang::En => "en",
        };
        let _ = s.set_item(LANG_KEY, val);
    }
}

/// Translate a key. Returns the translation for the given language.
/// Panics in debug mode if key is not found; returns "???" in release.
pub fn t(lang: Lang, key: &str) -> &'static str {
    let map = translations();
    match map.get(key) {
        Some((sk, en)) => match lang {
            Lang::Sk => sk,
            Lang::En => en,
        },
        None => {
            #[cfg(debug_assertions)]
            web_sys::console::warn_1(&format!("i18n: missing key '{key}'").into());
            "???"
        }
    }
}

/// Pick a joined service's display name for the active language. `None`
/// when the row wasn't tied to a service (e.g. a plain top-up). Single
/// source shared by the admin `TxnInfo::service_label` (dashboard/mod.rs)
/// and the customer `RecentTx::service_label` (my_balance.rs, #147) — same
/// joined `services.name_sk`/`name_en` columns, two response structs.
pub fn service_label<'a>(
    name_sk: &'a Option<String>,
    name_en: &'a Option<String>,
    lang: Lang,
) -> Option<&'a str> {
    match lang {
        Lang::Sk => name_sk.as_deref(),
        Lang::En => name_en.as_deref(),
    }
}

/// The i18n key for a transaction's `EventKind` label. Single source of the
/// mapping shared by the admin transactions list and the customer movements
/// list, so both surfaces show the same label for the same kind. Adding an
/// `EventKind` variant is a compile error here (exhaustive match).
pub fn tx_label_key(kind: spinbike_core::reports::EventKind) -> &'static str {
    use spinbike_core::reports::EventKind;
    match kind {
        EventKind::PassSale => "tx_label_pass",
        EventKind::Visit => "tx_label_visit",
        EventKind::Charge => "tx_label_charge",
        EventKind::TopUp => "tx_label_topup",
        EventKind::Other => "event_other",
    }
}

/// Format a `NaiveDate` for display, locale-aware.
/// Slovak: `DD.MM.YYYY` (e.g. `25.04.2026`). English: `YYYY-MM-DD` (ISO).
/// API request bodies and `<input type="date">` values must continue to use
/// the ISO form (`%Y-%m-%d`) explicitly — this helper is for display only.
pub fn fmt_date(d: chrono::NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::Sk => crate::dates::format_ddmmyyyy(d),
        Lang::En => d.format("%Y-%m-%d").to_string(),
    }
}

/// Short form of `fmt_date` (no year). Slovak: `25.04.`, English: `04-25`.
pub fn fmt_date_short(d: chrono::NaiveDate, lang: Lang) -> String {
    match lang {
        Lang::Sk => d.format("%d.%m.").to_string(),
        Lang::En => d.format("%m-%d").to_string(),
    }
}

/// 2-letter weekday abbreviation in target language.
/// Slovak: Po/Ut/St/Št/Pi/So/Ne · English: Mon/Tue/Wed/Thu/Fri/Sat/Sun.
pub fn fmt_weekday_short(d: chrono::NaiveDate, lang: Lang) -> &'static str {
    use chrono::Datelike;
    let wd = d.weekday().num_days_from_monday() as usize;
    match lang {
        Lang::Sk => ["Po", "Ut", "St", "\u{160}t", "Pi", "So", "Ne"][wd],
        Lang::En => ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][wd],
    }
}

/// Parse a server timestamp and resolve it to a Europe/Bratislava `DateTime`.
///
/// SQLite `datetime('now')` and ISO 8601 strings are interpreted as UTC and
/// converted via the IANA tz database (DST-aware). Legacy MS Access
/// `MM/DD/YY` patterns come from the old VB6 app on a Slovak PC and are
/// already local. Returns `None` if no pattern matches.
///
/// Returning a tz-aware `DateTime<Tz>` (rather than a `NaiveDateTime`) keeps
/// the timezone identity attached so future callers can do arithmetic or
/// comparisons without losing context. The display helpers below format
/// directly off the tz-aware value, which prints in local wall-clock time.
pub fn parse_to_local(s: &str) -> Option<chrono::DateTime<chrono_tz::Tz>> {
    use chrono::TimeZone;
    let trimmed = s.trim();
    let bratislava = chrono_tz::Europe::Bratislava;

    let utc_patterns = [
        "%Y-%m-%d %H:%M:%S",    // SQLite datetime('now')
        "%Y-%m-%dT%H:%M:%S",    // ISO 8601 with T
        "%Y-%m-%d %H:%M:%S%.f", // SQLite with fractional seconds
    ];
    for pattern in utc_patterns {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, pattern) {
            return Some(bratislava.from_utc_datetime(&naive));
        }
    }

    let local_patterns = [
        "%m/%d/%y %H:%M:%S", // legacy MS Access, 2-digit year
        "%m/%d/%Y %H:%M:%S", // legacy MS Access, 4-digit year
    ];
    for pattern in local_patterns {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(trimmed, pattern) {
            // Resolve in local time. Ambiguous fall-back overlap → pick
            // earliest (CEST, the earlier UTC instant). Non-existent
            // spring-forward gap shouldn't occur in legacy data, but if it
            // does, fall back to interpreting as UTC so we still render
            // something rather than dropping the row.
            return bratislava
                .from_local_datetime(&naive)
                .earliest()
                .or_else(|| Some(bratislava.from_utc_datetime(&naive)));
        }
    }

    None
}

/// Log a console warning for an unparseable server timestamp. WASM-only —
/// host-side `cargo test` builds compile this out so unit tests can exercise
/// the None branch without linking JS interop.
#[cfg(target_arch = "wasm32")]
fn warn_unparseable(helper: &str, s: &str) {
    web_sys::console::warn_1(&format!("i18n::{helper}: unparseable timestamp '{s}'").into());
}
#[cfg(not(target_arch = "wasm32"))]
fn warn_unparseable(_helper: &str, _s: &str) {}

/// Format a server timestamp as date + time, per-locale, in Slovak local time.
/// Slovak: `dd.mm.yyyy HH:MM` · English: `yyyy-mm-dd HH:MM`. Returns the
/// original string unchanged (and emits a console warning) if it doesn't
/// match any known pattern.
pub fn fmt_datetime_str(s: &str, lang: Lang) -> String {
    match parse_to_local(s) {
        Some(dt) => match lang {
            Lang::Sk => dt.format("%d.%m.%Y %H:%M").to_string(),
            Lang::En => dt.format("%Y-%m-%d %H:%M").to_string(),
        },
        None => {
            warn_unparseable("fmt_datetime_str", s);
            s.to_string()
        }
    }
}

/// Format a server timestamp as just `HH:MM` in Slovak local time. Used in
/// the activity feed where the date is supplied by the page-level day anchor.
/// Returns an empty string (and emits a console warning) on parse failure —
/// safer than the prior raw-split fallback that would have rendered a
/// fragment of the malformed input.
pub fn fmt_time_str(s: &str) -> String {
    match parse_to_local(s) {
        Some(dt) => dt.format("%H:%M").to_string(),
        None => {
            warn_unparseable("fmt_time_str", s);
            String::new()
        }
    }
}

/// The i18n key for a customer-facing API `error_code` (#145), when one
/// exists. Only CUSTOMER-facing codes get a translation — staff/admin-only
/// codes (`staff_required`, conflict codes, etc.) return `None` so the
/// caller falls back to the server's raw English `error` message, exactly
/// as before this ticket. Exhaustive match (like `tx_label_key` above) so a
/// new `ErrorCode` variant is a compile error here until someone decides
/// whether it needs a customer-facing translation.
pub fn error_code_key(code: spinbike_core::errors::ErrorCode) -> Option<&'static str> {
    use spinbike_core::errors::ErrorCode;
    match code {
        ErrorCode::InvalidCredentials => Some("err_invalid_credentials"),
        ErrorCode::OauthAccount => Some("err_oauth_account"),
        ErrorCode::BookingNotFound => Some("err_booking_not_found"),
        ErrorCode::BookingNotOwned => Some("err_booking_not_owned"),
        ErrorCode::UserNotFound => Some("err_user_not_found"),
        ErrorCode::Internal => Some("err_internal"),
        // Staff/admin-only or not customer-facing in the 5 render sites this
        // ticket scopes to (#145) — left unmapped on purpose, NOT an
        // oversight. Falls back to the server's raw English `error` text,
        // unchanged from pre-#145 behavior.
        ErrorCode::InvalidOrExpiredLink
        | ErrorCode::StaffRequired
        | ErrorCode::AdminRequired
        | ErrorCode::CardCodeStaffOnly
        | ErrorCode::AllowSelfEntryAdminOnly
        | ErrorCode::PasswordAdminOnly
        | ErrorCode::UserBlocked
        | ErrorCode::TransactionNotFound
        | ErrorCode::TransactionAlreadyVoided
        | ErrorCode::ServiceNotFound
        | ErrorCode::EmailConflict
        | ErrorCode::CardCodeConflict
        | ErrorCode::EmailOrCardConflict
        | ErrorCode::ClassFull
        | ErrorCode::ClassCancelled
        | ErrorCode::NoteOnVoidedTransaction
        | ErrorCode::DateOnVoidedTransaction
        | ErrorCode::NoActiveMonthlyPass
        | ErrorCode::MonthlyPassExists
        | ErrorCode::UserAlreadyDeleted
        // #143: the staff conflict-resolution dialog builds its own copy from
        // the conflict fields (name / date) + dedicated i18n keys — no single
        // localized banner string, so no key here.
        | ErrorCode::EmailBelongsToDeletedAccount
        | ErrorCode::BadRequest
        | ErrorCode::MailNotConfigured => None,
    }
}

/// Localize an API error banner for display: map `error_code` (when present
/// AND a customer-facing code per [`error_code_key`]) through `t`; otherwise
/// fall back to the server's raw `error` message — safe for staff/admin-only
/// codes and for any error the server didn't attach a code to at all.
pub fn localize_api_error(
    lang: Lang,
    code: Option<spinbike_core::errors::ErrorCode>,
    raw_message: &str,
) -> String {
    code.and_then(error_code_key)
        .map(|key| t(lang, key).to_string())
        .unwrap_or_else(|| raw_message.to_string())
}

/// Format a translated string with dynamic values. Returns an owned String.
pub fn tf(lang: Lang, key: &str, args: &[&str]) -> String {
    let template = t(lang, key);
    let mut result = template.to_string();
    for arg in args {
        if let Some(pos) = result.find("{}") {
            result.replace_range(pos..pos + 2, arg);
        }
    }
    result
}

// (sk, en)
type TransMap = HashMap<&'static str, (&'static str, &'static str)>;

static TRANSLATIONS: LazyLock<TransMap> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // Navigation
    m.insert("schedule", ("Rozvrh hodin", "Class Schedule"));
    m.insert("login", ("Prihlasenie", "Login"));
    m.insert("logout", ("Odhlasit", "Logout"));
    m.insert("my_bookings", ("Moje rezervacie", "My Bookings"));
    m.insert("balance", ("Zostatok", "Balance"));
    m.insert("staff", ("Personal", "Staff"));
    m.insert("admin", ("Administracia", "Admin"));

    // Auth forms
    m.insert("email", ("Email", "Email"));
    m.insert("password", ("Heslo", "Password"));
    m.insert("name", ("Meno", "Name"));
    m.insert("phone", ("Telefon", "Phone"));
    m.insert("company", ("Firma", "Company"));
    m.insert(
        "transaction_history",
        ("Historia transakcii", "Transaction History"),
    );
    m.insert(
        "no_transactions_card",
        ("Ziadne transakcie", "No transactions"),
    );
    m.insert("date", ("Datum", "Date"));
    m.insert("logging_in", ("Prihlasovanie...", "Logging in..."));

    // Magic-link onboarding (/welcome + login-page customer section, #109)
    m.insert("welcome_loading", ("Prihlasujem...", "Signing you in..."));
    m.insert("welcome_title", ("Vitaj v SpinBike", "Welcome to SpinBike"));
    m.insert(
        "welcome_message",
        (
            "Uspesne prihlaseny. Tvoj zostatok a otvaranie dveri najdes tu:",
            "You're logged in. Find your balance and door access here:",
        ),
    );
    m.insert(
        "welcome_cta",
        ("Prejst na moj zostatok", "Go to my balance"),
    );
    m.insert(
        "welcome_invalid_title",
        ("Odkaz nie je platny", "This link isn't valid"),
    );
    m.insert(
        "welcome_invalid_message",
        (
            "Odkaz je bud neplatny, expirovany, alebo uz bol pouzity. Zadaj svoj email a poslime ti novy.",
            "This link is invalid, expired, or already used. Enter your email and we'll send you a new one.",
        ),
    );

    // Install-to-home-screen prompt (#110)
    m.insert(
        "install_prompt_cta",
        ("Pridat na plochu", "Add to home screen"),
    );
    m.insert(
        "install_prompt_ios_title",
        (
            "Pridaj SpinBike na plochu",
            "Add SpinBike to your home screen",
        ),
    );
    m.insert(
        "install_prompt_ios_step1",
        (
            "Tukni na Zdielat (ikona so sipkou)",
            "Tap Share (the arrow icon)",
        ),
    );
    m.insert(
        "install_prompt_ios_step2",
        (
            "Vyber \"Pridat na plochu\"",
            "Choose \"Add to Home Screen\"",
        ),
    );
    // iOS install guide v2 (#226): scroll hint under the numbered steps,
    // a permanent footer fallback (for a webview UA sniffing can't catch,
    // e.g. SFSafariViewController), and the separate webview-detected branch
    // (open-in-Safari instruction + copy-URL button).
    m.insert(
        "install_prompt_ios_scroll_hint",
        (
            "Ak moznost nevidis, posun zoznam nizsie",
            "If you don't see it, scroll down in the list",
        ),
    );
    m.insert(
        "install_prompt_ios_footer_hint",
        (
            "Nejde to? Otvor tuto stranku priamo v Safari.",
            "Not working? Open this page directly in Safari.",
        ),
    );
    m.insert(
        "install_prompt_webview_title",
        (
            "Instalacia tu nie je mozna - otvor stranku v Safari",
            "Installation isn't possible here - open the page in Safari",
        ),
    );
    m.insert(
        "install_prompt_copy_button",
        ("Kopirovat odkaz", "Copy link"),
    );
    m.insert(
        "install_prompt_copy_confirm",
        ("Odkaz skopirovany", "Link copied"),
    );
    m.insert(
        "customer_login_heading",
        ("Prihlasenie pre klientov", "Customer login"),
    );
    // #151: static, response-INDEPENDENT hint — shown unconditionally
    // regardless of what (or whether) the user has typed, so it adds zero
    // user-enumeration surface. Clarifies why an admin/staff email typed
    // into this field silently produces no email (request-login-link only
    // sends for role=customer accounts, but always returns 200 by design).
    m.insert(
        "login_link_customer_only_help",
        (
            "Tento odkaz je len pre zakaznicke ucty. Personal a admin sa prihlasuju heslom vyssie.",
            "This link is for client accounts only. Staff and admin log in with a password above.",
        ),
    );
    m.insert(
        "send_login_link",
        ("Poslat prihlasovaci link", "Send login link"),
    );
    m.insert("sending_login_link", ("Odosielam...", "Sending..."));
    m.insert(
        "login_link_sent",
        (
            "Ak email existuje, poslali sme prihlasovaci odkaz",
            "If that email exists, we sent a login link",
        ),
    );

    // Class card / schedule
    m.insert("book", ("Rezervovat", "BOOK"));
    m.insert("booked", ("REZERVOVANE", "BOOKED"));
    m.insert("full", ("OBSADENE", "FULL"));
    m.insert("cancelled", ("Zrusene", "Cancelled"));
    m.insert("cancel", ("Zrusit", "Cancel"));
    m.insert("cancel_booking", ("Zrusit rezervaciu", "Cancel booking"));
    m.insert("cancel_class", ("Zrusit hodinu", "Cancel Class"));
    m.insert("spots_format", ("{}/{} miest", "{}/{} spots"));
    m.insert("instructor_format", ("Instruktor #{}", "Instructor #{}"));
    m.insert(
        "login_to_book",
        ("Pre rezervaciu sa prihlaste", "Login to book"),
    );
    m.insert(
        "no_classes_today",
        (
            "Dnes nie su naplanovane hodiny",
            "No classes scheduled for this day",
        ),
    );
    m.insert(
        "no_classes_week",
        ("Ziadne hodiny tento tyzden", "No classes this week"),
    );

    // Day names (short)
    m.insert("mon", ("Po", "Mon"));
    m.insert("tue", ("Ut", "Tue"));
    m.insert("wed", ("St", "Wed"));
    m.insert("thu", ("\u{160}t", "Thu"));
    m.insert("fri", ("Pi", "Fri"));
    m.insert("sat", ("So", "Sat"));
    m.insert("sun", ("Ne", "Sun"));

    // My bookings
    m.insert(
        "no_bookings",
        ("Ziadne nadchadzajuce rezervacie", "No upcoming bookings"),
    );

    // My balance
    m.insert("my_balance", ("Moj zostatok", "My Balance"));

    // Card operations
    m.insert("activate", ("Aktivovat", "Activate"));
    m.insert(
        "all_member_cards",
        ("Vsetky clenske karty", "All Member Cards"),
    );
    m.insert("active", ("Aktivna", "Active"));
    m.insert("inactive", ("Neaktivna", "Inactive"));
    m.insert("blocked", ("Zablokovana", "Blocked"));
    m.insert("topup", ("Dobit", "Top Up"));
    m.insert("block", ("Zablokovat", "Block"));
    m.insert("unblock", ("Odblokovat", "Unblock"));
    m.insert("card_code", ("Kod karty", "Card code"));
    m.insert(
        "new_card_barcode",
        ("Ciarovy kod novej karty", "New card barcode"),
    );

    m.insert(
        "search_cards_placeholder",
        (
            "Hladaj podla mena, firmy, telefonu, alebo ciarkoveho kodu…",
            "Search by name, company, phone, or barcode…",
        ),
    );
    m.insert("searching", ("Hladam…", "Searching…"));
    m.insert("no_matches", ("Ziadne zhody", "No matches"));
    m.insert(
        "topup_ok_format",
        ("Dobite! Novy kredit: {} €", "Topped up! New credit: {} €"),
    );
    m.insert(
        "charge_ok_format",
        ("Uctovane. Zostatok: {} €", "Charged. Balance: {} €"),
    );
    m.insert(
        "visit_added_format",
        ("Vstup pridany: {}", "Visit added: {}"),
    );
    m.insert("block_ok", ("Karta zablokovana", "Card blocked"));
    m.insert("unblock_ok", ("Karta odblokovana", "Card unblocked"));
    m.insert("saved", ("Ulozene", "Saved"));

    // Staff dashboard
    m.insert("staff_dashboard", ("Panel personalu", "Staff Dashboard"));
    m.insert("add_walk_in", ("+ Navstevnik", "+ Walk-in"));
    m.insert("booked_format", ("{}/{} rezervovanych", "{}/{} booked"));
    m.insert(
        "enter_valid_user_id",
        ("Zadajte platne ID pouzivatela", "Enter a valid user ID"),
    );
    m.insert("search_card", ("Hladat kartu", "Search card"));
    m.insert(
        "search_card_placeholder",
        (
            "Meno, priezvisko alebo ciarovy kod",
            "Name, surname or barcode",
        ),
    );

    // Payments
    m.insert("charge", ("Platba", "Charge"));
    m.insert("select_service", ("-- Vyberte --", "-- Select --"));
    m.insert("amount", ("Suma", "Amount"));
    m.insert("price", ("Cena", "Price"));

    // Admin
    m.insert("templates", ("Sablony hodin", "Templates"));
    m.insert("instructors", ("Instruktori", "Instructors"));
    m.insert("services", ("Sluzby", "Services"));
    m.insert("users", ("Pouzivatelia", "Users"));
    m.insert("settings", ("Nastavenia", "Settings"));
    m.insert("weekday", ("Den", "Weekday"));
    m.insert("start_time", ("Zaciatok", "Start Time"));
    m.insert("duration", ("Trvanie", "Duration"));
    m.insert("capacity", ("Kapacita", "Capacity"));
    m.insert("instructor_id", ("ID instruktora", "Instructor ID"));
    m.insert("create", ("Vytvorit", "Create"));
    m.insert("edit", ("Upravit", "Edit"));
    m.insert("save", ("Ulozit", "Save"));
    m.insert("delete", ("Vymazat", "Delete"));
    m.insert("deactivate", ("Deaktivovat", "Deactivate"));
    m.insert("role", ("Rola", "Role"));
    m.insert("key", ("Kluc", "Key"));
    m.insert("value", ("Hodnota", "Value"));
    m.insert("add_instructor", ("Pridat instruktora", "Add Instructor"));
    m.insert("add_service", ("Pridat sluzbu", "Add Service"));
    m.insert("time", ("Cas", "Time"));
    m.insert("optional", ("Volitelne", "Optional"));

    // Service catalog (V8 dual-language)
    m.insert("service_name_sk", ("Slovensky nazov", "Slovak name"));
    m.insert("service_name_en", ("Anglicky nazov", "English name"));
    m.insert("kind", ("Druh", "Kind"));
    m.insert("service_kind_generic", ("Polozka", "Item"));
    m.insert(
        "service_kind_monthly_pass",
        ("Mesacny listok", "Monthly pass"),
    );
    m.insert(
        "service_kind_single_entry",
        ("Jednorazovy vstup", "Single entry"),
    );

    // Monthly pass banner
    // #32: collapsed single-line pass status (active + expired). Used by
    // pass_banner.rs. Placeholders are sequential `{}` per i18n::tf — first
    // `{}` is the date, second `{}` is the day count (active form).
    // For the expired form, first `{}` is days-ago count, second `{}` is the
    // last-valid date.
    m.insert(
        "pass_active_oneline_format",
        (
            "✓ Mesacny listok do {} ({} dni)",
            "✓ Monthly pass valid until {} ({} days)",
        ),
    );
    m.insert(
        "pass_expired_oneline_format",
        (
            "⚠ Mesacny listok vyprsal pred {} dnami (do {})",
            "⚠ Monthly pass expired {} days ago (was valid until {})",
        ),
    );
    m.insert(
        "edit_pass_date",
        ("Zmenit koniec permanentky", "Change pass end date"),
    );

    // Transaction history void
    m.insert("void", ("Zrusit", "Void"));
    m.insert("voided", ("zrusene", "voided"));
    m.insert(
        "confirm_void",
        (
            "Zrusit tento zaznam? Neda sa vratit.",
            "Void this entry? This cannot be undone from the UI.",
        ),
    );

    // Visit logging (active pass flow)
    m.insert("log_visit", ("Zaznamenat navstevu", "Log visit"));
    m.insert(
        "charge_for_extras",
        (
            "Platba za napoje / jedlo / ine",
            "Charge for drinks / food / other",
        ),
    );

    // Transaction history action labels (EventKind-driven; DB stores raw English: topup/charge/visit/pass).
    m.insert("tx_label_topup", ("Dobitie kreditu", "Top-up"));
    m.insert("tx_label_charge", ("Vydaj z kreditu", "Spent from credit"));
    m.insert(
        "tx_label_visit",
        ("Vstup s permanentkou", "Entry with pass"),
    );
    m.insert("tx_label_pass", ("Predaj permanentky", "Sale of pass"));
    // Transaction note UI strings
    m.insert(
        "tx_note_placeholder",
        ("Poznamka (nepovinne)", "Note (optional)"),
    );
    m.insert("tx_note_edit", ("Upravit poznamku", "Edit note"));
    m.insert("tx_note_save", ("Ulozit", "Save"));
    m.insert("tx_note_cancel", ("Zrusit", "Cancel"));
    m.insert("tx_until_short", ("do", "until"));
    m.insert("error_format", ("Chyba: {}", "Error: {}"));

    // Sell pass modal
    m.insert(
        "sell_monthly_pass",
        ("Predat mesacny listok", "Sell monthly pass"),
    );
    m.insert("modal_date", ("Datum", "Date"));
    m.insert("modal_valid_until", ("Platny do", "Valid until"));
    m.insert("modal_confirm", ("Potvrdit", "OK"));
    m.insert("modal_cancel", ("Zrusit", "Cancel"));
    m.insert("sell_pass_action", ("Predat", "Sell pass"));
    m.insert("price_required", ("Zadajte cenu", "Please enter a price"));

    // Upcoming classes + persistent booking
    m.insert(
        "upcoming_classes",
        ("Nadchadzajuce hodiny", "Upcoming classes"),
    );
    m.insert(
        "persistent_booking",
        ("Trvala rezervacia", "Persistent booking"),
    );
    m.insert("auto", ("AUTO", "AUTO"));
    m.insert(
        "skip_this_week",
        ("Preskocit tento tyzden", "Skip this week"),
    );
    m.insert("past", ("UPLYNULE", "PAST"));
    m.insert("turn_on", ("Zapnut", "On"));
    m.insert("turn_off", ("Vypnut", "Off"));

    // Card detail tabs
    m.insert("tab_history", ("Historia", "History"));
    m.insert("tab_upcoming", ("Pripravovane", "Upcoming"));
    m.insert("tab_persistent", ("Opakovane", "Persistent"));
    m.insert("tab_overview", ("Prehlad", "Overview"));

    // Overview tab — KPI table + bar charts
    m.insert("overview_period_month", ("Tento mesiac", "This month"));
    m.insert("overview_period_year", ("Tento rok", "This year"));
    m.insert("overview_period_all", ("Spolu", "All time"));
    m.insert("overview_col_visits", ("Vstupy", "Visits"));
    m.insert("overview_col_topup", ("Dobitie", "Topped up"));
    m.insert(
        "overview_chart_visits",
        ("Vstupy po mesiacoch", "Visits per month"),
    );
    m.insert(
        "overview_chart_topup",
        (
            "Dobitie po mesiacoch (\u{20ac})",
            "Topped up per month (\u{20ac})",
        ),
    );
    m.insert("overview_loading", ("Nacitavam stat...", "Loading..."));

    // General
    m.insert("page_not_found", ("Stranka nenajdena", "Page not found"));
    m.insert(
        "unable_to_load",
        ("Nepodarilo sa nacitat", "Unable to load"),
    );

    // Redesign 2026: new UI strings
    m.insert("show_older", ("Zobrazit starsie", "Show older"));
    m.insert("close", ("Zatvorit", "Close"));
    m.insert("edit_info", ("Upravit udaje", "Edit info"));

    // Staff invite button in edit-info form (#111)
    m.insert("send_invite", ("Poslat pozvanku", "Send invite"));
    m.insert(
        "invite_sent",
        ("Pozvanka odoslana na {}", "Invite sent to {}"),
    );
    m.insert(
        "invite_mail_not_configured",
        (
            "Odosielanie emailov nie je nakonfigurovane",
            "Email sending is not configured",
        ),
    );
    m.insert(
        "sell_pass_label",
        ("Predat mesacny preukaz", "Sell monthly pass"),
    );
    m.insert("edit_pass_date", ("Upravit datum", "Edit date"));
    m.insert(
        "edit_tx_date",
        ("Zmenit datum zaznamu", "Change entry date"),
    );
    m.insert("tx_date_edit_tooltip", ("Zmenit datum", "Change date"));
    m.insert(
        "tx_date_window_error",
        (
            "Datum musi byt v poslednych 30 dnoch",
            "Date must be within last 30 days",
        ),
    );

    // --- Staff/CEO redesign (v0.10.0) ---

    // Nav (bottom tabs + sidebar labels)
    m.insert("nav_desk", ("Desk", "Desk"));
    m.insert("nav_schedule", ("Plan", "Schedule"));
    m.insert("nav_reports", ("Prehlad", "Reports"));
    m.insert("nav_settings", ("Nastavenia", "Settings"));
    m.insert("nav_more", ("Viac", "More"));

    // Reports — date navigation
    m.insert("reports_yesterday", ("Vcera", "Yesterday"));
    m.insert("reports_today", ("Dnes", "Today"));
    m.insert("reports_week", ("Tyzden", "Week"));
    m.insert("reports_month", ("Mesiac", "Month"));
    m.insert("reports_pick_date", ("Zvolit datum", "Pick date"));

    // Reports — KPI cards
    m.insert("kpi_spinning_visits", ("SPINNING", "SPINNING"));
    m.insert("kpi_attendance", ("NAVSTEVY", "ATTENDANCE"));
    m.insert("kpi_passes", ("PERMANENTKY", "PASSES"));
    m.insert("kpi_cash_in", ("VKLADY", "CASH IN"));

    // Reports — filters
    m.insert("filters_label", ("Filtre", "Filters"));
    m.insert("filters_reset", ("Zrusit filtre", "Reset"));
    m.insert("filters_event_all", ("Vsetko", "All"));
    m.insert("filters_event_payments", ("Platby", "Payments"));
    m.insert("filters_event_topups", ("Vklady", "Top-ups"));
    m.insert("filters_event_passes", ("Permanentky", "Passes"));
    m.insert(
        "filters_search_placeholder",
        (
            "Hladat meno, ciarovy kod, telefon",
            "Search name, barcode, phone",
        ),
    );

    // Reports — feed event labels (Other is a fallback; main labels come from tx_label_*)
    m.insert("event_other", ("Ine", "Other"));
    // Reports — feed
    m.insert("feed_load_older", ("Nacitat starsie", "Load older"));
    m.insert(
        "feed_empty_day",
        (
            "Na tento den nie je ziadna aktivita.",
            "No activity on this day.",
        ),
    );
    m.insert(
        "feed_empty_filter",
        (
            "Ziadne vysledky pre tieto filtre.",
            "No results for these filters.",
        ),
    );

    // Card detail (collapsed contact)
    m.insert("card_show_contact", ("Zobrazit kontakt", "Show contact"));
    m.insert("card_hide_contact", ("Skryt kontakt", "Hide contact"));

    // Settings (renamed /admin) tabs
    m.insert("settings_tab_center", ("Centrum", "Center"));
    m.insert("settings_tab_services", ("Sluzby", "Services"));
    m.insert("settings_tab_templates", ("Permanentky", "Templates"));
    m.insert("settings_tab_instructors", ("Instruktori", "Instructors"));
    m.insert("settings_tab_users", ("Pouzivatelia", "Users"));

    // Relative-time labels (last visit display — #57)
    // Note: `rel_days_one` and `rel_months_one` are currently UNREACHABLE under
    // the bucket logic in `relative_date::relative` — `days==1` short-circuits
    // to "yesterday" before reaching the days bucket, and the months bucket
    // starts at days=61 where N is always >=2. Kept for symmetry with the
    // `_few` forms in case a future bucket adjustment makes them reachable.
    m.insert("last_visit_label", ("Posledna navsteva", "Last visit"));

    // Negative-balance list (#49)
    m.insert(
        "negative_balance_heading",
        ("Klienti v minuse", "Customers with negative balance"),
    );
    m.insert("last_payment_label", ("Posledna platba", "Last payment"));
    m.insert("never_label", ("nikdy", "never"));
    m.insert("rel_today", ("dnes", "today"));
    m.insert("rel_yesterday", ("vcera", "yesterday"));
    m.insert("rel_days_one", ("pred 1 dnom", "1 day ago"));
    m.insert("rel_days_few", ("pred {n} dnami", "{n} days ago"));
    m.insert("rel_weeks_one", ("pred 1 tyzdnom", "1 week ago"));
    m.insert("rel_weeks_few", ("pred {n} tyzdnami", "{n} weeks ago"));
    m.insert("rel_months_one", ("pred 1 mesiacom", "1 month ago"));
    m.insert("rel_months_few", ("pred {n} mesiacmi", "{n} months ago"));
    m.insert("rel_years_one", ("pred 1 rokom", "1 year ago"));
    m.insert("rel_years_few", ("pred {n} rokmi", "{n} years ago"));

    // Add person form (#55)
    m.insert("add_person", ("Pridat osobu", "Add Person"));
    m.insert("hide_add_person", ("Skryt formular", "Hide form"));
    m.insert("add_person_submit", ("Ulozit osobu", "Save Person"));
    m.insert(
        "add_person_ok_format",
        ("Osoba pridana: {}", "Person added: {}"),
    );
    m.insert("name_required", ("Meno je povinne", "Name is required"));
    m.insert("optional_paren", ("(volitelne)", "(optional)"));

    // Reports tabs + users-by-movement report + delete-user modal (#56)
    m.insert("reports_tab_daily", ("Denna aktivita", "Daily activity"));
    m.insert("reports_tab_users", ("Pouzivatelia", "Users"));
    m.insert(
        "users_by_movement_heading",
        (
            "Pouzivatelia podla posledneho pohybu",
            "Users by last movement",
        ),
    );
    m.insert("no_movement_yet", ("Bez pohybu", "No movement yet"));
    m.insert("show_more", ("Zobrazit dalsie", "Show more"));
    m.insert("delete_user", ("Zmazat pouzivatela", "Delete user"));
    m.insert(
        "delete_user_confirm_title",
        ("Zmazat {name}?", "Delete {name}?"),
    );
    m.insert(
        "delete_user_confirm_body",
        (
            "Tato akcia skryje pouzivatela vsade. Historia ostane v DB.",
            "Hides the user everywhere. History stays in the DB.",
        ),
    );
    m.insert(
        "delete_user_warning_balance",
        ("Zostatok: {amount} EUR", "Balance: {amount} EUR"),
    );
    m.insert(
        "delete_user_warning_pass",
        (
            "Aktivna permanentka do {date}",
            "Active permanentka until {date}",
        ),
    );
    m.insert("delete_user_cancel", ("Zrusit", "Cancel"));
    m.insert("delete_user_confirm", ("Zmazat", "Delete"));

    // Door self-entry (#92)
    m.insert(
        "door_button_idle",
        ("Otvorit dvere - drz 2s", "Hold to open door"),
    );
    m.insert("door_button_holding", ("Drz...", "Hold..."));
    m.insert("door_button_firing", ("Otvaram...", "Opening..."));
    m.insert(
        "door_success",
        ("Dvere otvorene - vojdi", "Door open - step in"),
    );
    m.insert(
        "door_unavailable",
        (
            "Dvere nedostupne - oslov recepciu",
            "Door unavailable - ask reception",
        ),
    );
    m.insert(
        "door_rate_limited",
        ("Pockaj chvilu...", "Wait a moment..."),
    );
    m.insert(
        "door_not_allowed",
        ("Oslov recepciu pre vstup", "Ask reception for entry"),
    );
    m.insert("door_lock_icon_aria", ("Ikona zamku", "Lock icon"));
    // Customer movements: localized display of the stored English "door: Nth" note.
    m.insert("door_note_reentry", ("Vstup c. {}", "Entry #{}"));
    m.insert(
        "version_footer_aria",
        ("Verzia aplikacie", "Application version"),
    );
    m.insert(
        "monthly_pass_active_until",
        (
            "Mesacny preplatok aktivny do {}",
            "Monthly pass active until {}",
        ),
    );
    m.insert(
        "monthly_pass_not_active",
        ("Mesacny preplatok neaktivny", "Monthly pass not active"),
    );
    m.insert("my_balance_hello", ("Ahoj, {}", "Hello, {}"));
    m.insert("my_balance_credit", ("Zostatok", "Credit"));
    m.insert(
        "my_balance_recent_movements",
        ("Posledne pohyby", "Recent activity"),
    );
    m.insert(
        "admin_allow_self_entry",
        ("Povolit samoobsluzny vstup", "Allow self-entry"),
    );
    m.insert(
        "admin_allow_self_entry_help",
        (
            "(otvaranie dveri z PWA bez pritomnosti personalu)",
            "(open door from PWA without staff present)",
        ),
    );
    m.insert("door_page_title", ("Otvorenie dveri", "Open door"));
    m.insert("user_edit_new_password", ("Nove heslo", "New password"));
    m.insert(
        "user_edit_new_password_placeholder",
        (
            "ponechaj prazdne pre zachovanie hesla",
            "leave blank to keep existing password",
        ),
    );
    m.insert(
        "user_edit_new_password_help",
        (
            "Min. 8 znakov. Iba admin moze menit cudzie heslo.",
            "Min 8 characters. Only admin can set another user's password.",
        ),
    );
    // Plain-Slovak rendering of the server's email-uniqueness 409 (the raw
    // server text is English). Shown in-sheet on a failed edit-save so the
    // operator understands WHY the email did not save.
    m.insert(
        "email_already_used",
        (
            "Tento email uz pouziva iny ucet. Jeden email moze patrit len jednemu uctu.",
            "This email is already used by another account. One email can belong to only one account.",
        ),
    );
    // Named variant: {} is the colliding account's name (+ card code), which
    // the server returns only to staff/admin. Lets the operator go find and
    // fix that account.
    m.insert(
        "email_already_used_by",
        (
            "Tento email uz pouziva ucet: {}. Jeden email moze patrit len jednemu uctu.",
            "This email is already used by account: {}. One email can belong to only one account.",
        ),
    );

    // #143 — soft-deleted-email conflict resolution dialog. When an email is
    // held by an ARCHIVED (soft-deleted) account, the desk gets a clear message
    // plus two explicit actions instead of an opaque error.
    m.insert(
        "deleted_email_conflict_title",
        (
            "Email patri zmazanemu uctu",
            "Email belongs to a deleted account",
        ),
    );
    // {} = account name, {} = deletion date.
    m.insert(
        "deleted_email_conflict_body",
        (
            "Tento email uz patri zmazanemu uctu: {} (zmazany {}). Vyber, ako pokracovat:",
            "This email belongs to a deleted account: {} (deleted {}). Choose how to continue:",
        ),
    );
    // Fallback when the deletion date is unavailable. {} = account name.
    m.insert(
        "deleted_email_conflict_body_nodate",
        (
            "Tento email uz patri zmazanemu uctu: {}. Vyber, ako pokracovat:",
            "This email belongs to a deleted account: {}. Choose how to continue:",
        ),
    );
    m.insert("deleted_email_restore", ("Obnovit ucet", "Restore account"));
    m.insert(
        "deleted_email_restore_help",
        (
            "Vrati povodny zmazany ucet aj s jeho historiou a kreditom. Tvoja rozpracovana zmena sa nedokonci.",
            "Brings back the original deleted account with its history and credit. Your pending change is not applied.",
        ),
    );
    m.insert("deleted_email_free", ("Uvolnit email", "Free the email"));
    m.insert(
        "deleted_email_free_help",
        (
            "Odstrani email zo zmazaneho uctu (ten ostane archivovany) a dokonci povodnu akciu.",
            "Removes the email from the deleted account (it stays archived) and completes the original action.",
        ),
    );
    m.insert(
        "deleted_email_restored_ok",
        (
            "Povodny ucet bol obnoveny.",
            "The original account was restored.",
        ),
    );

    // Customer-facing API error banners (#145). Keyed off
    // `spinbike_core::errors::ErrorCode` (#158) via `error_code_key` above —
    // only the codes a customer can actually hit at the 5 scoped render
    // sites (login, my-balance, my-bookings, door, login-link-form) get a
    // translation; everything else falls back to the server's raw English
    // `error` text.
    m.insert(
        "err_invalid_credentials",
        ("Nespravny email alebo heslo", "Invalid email or password"),
    );
    // `oauth_account` fires when `password_hash` is NULL (login.rs uses the
    // password form against an account that has no password set). This repo
    // has no actual third-party OAuth button wired into the UI today — the
    // code name is legacy/forward-looking scaffolding (see
    // crates/spinbike-server/src/auth/oauth.rs) — so a specific provider
    // name would be misleading. Kept deliberately generic.
    m.insert(
        "err_oauth_account",
        (
            "Tento ucet pouziva ine prihlasenie",
            "This account uses a different sign-in method",
        ),
    );
    m.insert(
        "err_booking_not_found",
        ("Rezervacia sa nenasla", "Booking not found"),
    );
    m.insert(
        "err_booking_not_owned",
        (
            "Nemozes zrusit cudziu rezervaciu",
            "You can't cancel someone else's booking",
        ),
    );
    m.insert(
        "err_user_not_found",
        ("Pouzivatel sa nenasiel", "User not found"),
    );
    m.insert(
        "err_internal",
        (
            "Nastala chyba, skus to prosim znova",
            "Something went wrong, please try again",
        ),
    );
    // Generic api.rs fallbacks (not error-code-driven — these fire for the
    // session-expiry redirect and for a response whose body carries no
    // `error` field at all). Customer-visible at any authenticated call
    // site, not just the 5 scoped render sites, so api.rs applies these via
    // `i18n::get_saved_lang()` directly rather than the reactive `Lang`
    // signal (api.rs has no Leptos component context).
    m.insert(
        "err_session_expired",
        (
            "Prihlasenie vyprsalo, presmerovavam...",
            "Session expired, redirecting to login...",
        ),
    );
    m.insert(
        "err_request_failed_format",
        ("Poziadavka zlyhala (HTTP {})", "Request failed (HTTP {})"),
    );

    m
});

fn translations() -> &'static TransMap {
    &TRANSLATIONS
}

/// Get the short day name keys in order Mon-Sun
pub static DAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Admin tab name translations (keys map to settings_tab_* in TRANSLATIONS).
pub static ADMIN_TAB_KEYS: [(&str, &str); 5] = [
    ("templates", "settings_tab_templates"),
    ("instructors", "settings_tab_instructors"),
    ("services", "settings_tab_services"),
    ("users", "settings_tab_users"),
    ("settings", "settings_tab_center"),
];

/// Weekday names used in admin (short) - same as DAY_KEYS
pub static WEEKDAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[cfg(test)]
mod datetime_tests {
    use super::{Lang, fmt_datetime_str, fmt_time_str};
    use wasm_bindgen_test::*;

    // UTC-source rows shift into Europe/Bratislava (CET = +1 winter,
    // CEST = +2 summer). 2026-04-14 18:13 UTC is in CEST → 20:13 local.
    #[wasm_bindgen_test]
    fn sqlite_format_sk_shifts_to_local() {
        assert_eq!(
            fmt_datetime_str("2026-04-14 18:13:11", Lang::Sk),
            "14.04.2026 20:13"
        );
    }

    #[wasm_bindgen_test]
    fn sqlite_format_en_shifts_to_local() {
        assert_eq!(
            fmt_datetime_str("2026-04-14 18:13:11", Lang::En),
            "2026-04-14 20:13"
        );
    }

    #[wasm_bindgen_test]
    fn iso_8601_shifts_to_local() {
        assert_eq!(
            fmt_datetime_str("2026-04-14T18:13:11", Lang::Sk),
            "14.04.2026 20:13"
        );
    }

    #[wasm_bindgen_test]
    fn fractional_seconds_shift_to_local() {
        assert_eq!(
            fmt_datetime_str("2026-04-14 18:13:11.123", Lang::Sk),
            "14.04.2026 20:13"
        );
    }

    // CET (winter): UTC + 1.
    #[wasm_bindgen_test]
    fn cet_winter_shift() {
        assert_eq!(
            fmt_datetime_str("2026-01-15 10:00:00", Lang::Sk),
            "15.01.2026 11:00"
        );
    }

    // CEST (summer): UTC + 2.
    #[wasm_bindgen_test]
    fn cest_summer_shift() {
        assert_eq!(
            fmt_datetime_str("2026-07-15 10:00:00", Lang::Sk),
            "15.07.2026 12:00"
        );
    }

    // Spring forward 2026: at 01:00 UTC on Sun Mar 29, local jumps 02:00→03:00.
    #[wasm_bindgen_test]
    fn dst_spring_forward_before() {
        // 00:30 UTC → CET 01:30 local
        assert_eq!(
            fmt_datetime_str("2026-03-29 00:30:00", Lang::Sk),
            "29.03.2026 01:30"
        );
    }

    #[wasm_bindgen_test]
    fn dst_spring_forward_after() {
        // 01:30 UTC → CEST 03:30 local (the 02:00–03:00 local window doesn't exist)
        assert_eq!(
            fmt_datetime_str("2026-03-29 01:30:00", Lang::Sk),
            "29.03.2026 03:30"
        );
    }

    // Fall back 2026: at 01:00 UTC on Sun Oct 25, local goes 03:00→02:00.
    #[wasm_bindgen_test]
    fn dst_fall_back_before() {
        // 00:30 UTC → CEST 02:30 local
        assert_eq!(
            fmt_datetime_str("2026-10-25 00:30:00", Lang::Sk),
            "25.10.2026 02:30"
        );
    }

    #[wasm_bindgen_test]
    fn dst_fall_back_after() {
        // 01:30 UTC → CET 02:30 local (the 02:00–03:00 local window repeats)
        assert_eq!(
            fmt_datetime_str("2026-10-25 01:30:00", Lang::Sk),
            "25.10.2026 02:30"
        );
    }

    // Legacy MS Access rows are already Slovak local time → no shift.
    #[wasm_bindgen_test]
    fn legacy_two_digit_year_unchanged() {
        assert_eq!(
            fmt_datetime_str("03/24/26 18:59:08", Lang::Sk),
            "24.03.2026 18:59"
        );
    }

    #[wasm_bindgen_test]
    fn legacy_four_digit_year_unchanged() {
        assert_eq!(
            fmt_datetime_str("03/24/2026 18:59:08", Lang::Sk),
            "24.03.2026 18:59"
        );
    }

    // A legacy timestamp during CEST window must still NOT shift — proves
    // the dual-path dispatch sends legacy inputs through the local branch
    // even when their date would otherwise look summer-time-eligible.
    #[wasm_bindgen_test]
    fn legacy_summer_date_does_not_shift() {
        assert_eq!(
            fmt_datetime_str("07/15/2026 10:00:00", Lang::Sk),
            "15.07.2026 10:00"
        );
    }

    #[wasm_bindgen_test]
    fn unknown_returns_input() {
        assert_eq!(fmt_datetime_str("not-a-date", Lang::Sk), "not-a-date");
    }

    #[wasm_bindgen_test]
    fn fmt_time_str_shifts_utc_to_local_summer() {
        // 10:00 UTC summer → 12:00 CEST
        assert_eq!(fmt_time_str("2026-07-15 10:00:00"), "12:00");
    }

    #[wasm_bindgen_test]
    fn fmt_time_str_shifts_utc_to_local_winter() {
        // 10:00 UTC winter → 11:00 CET
        assert_eq!(fmt_time_str("2026-01-15 10:00:00"), "11:00");
    }

    #[wasm_bindgen_test]
    fn fmt_time_str_legacy_unchanged() {
        // Legacy MS-Access timestamp is already local — no shift.
        assert_eq!(fmt_time_str("07/15/2026 10:00:00"), "10:00");
    }

    #[wasm_bindgen_test]
    fn fmt_time_str_unknown_returns_empty() {
        assert_eq!(fmt_time_str("not-a-date"), "");
    }
}

#[cfg(test)]
mod format_key_tests {
    use super::{Lang, tf};
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    #[wasm_bindgen_test]
    fn visit_added_format_renders_slovak() {
        assert_eq!(
            tf(Lang::Sk, "visit_added_format", &["Fitness"]),
            "Vstup pridany: Fitness"
        );
    }

    #[wasm_bindgen_test]
    fn visit_added_format_renders_english() {
        assert_eq!(
            tf(Lang::En, "visit_added_format", &["Fitness"]),
            "Visit added: Fitness"
        );
    }
}

#[cfg(test)]
mod neg_balance_tests {
    use super::{Lang, t};
    use wasm_bindgen_test::*;

    // No wasm_bindgen_test_configure! — CI uses wasm-pack test --node (not browser).

    #[wasm_bindgen_test]
    fn negative_balance_heading_slovak() {
        assert_eq!(t(Lang::Sk, "negative_balance_heading"), "Klienti v minuse");
    }

    #[wasm_bindgen_test]
    fn negative_balance_heading_english() {
        assert_eq!(
            t(Lang::En, "negative_balance_heading"),
            "Customers with negative balance"
        );
    }

    #[wasm_bindgen_test]
    fn last_payment_label_slovak() {
        assert_eq!(t(Lang::Sk, "last_payment_label"), "Posledna platba");
    }

    #[wasm_bindgen_test]
    fn last_payment_label_english() {
        assert_eq!(t(Lang::En, "last_payment_label"), "Last payment");
    }

    #[wasm_bindgen_test]
    fn never_label_slovak() {
        assert_eq!(t(Lang::Sk, "never_label"), "nikdy");
    }

    #[wasm_bindgen_test]
    fn never_label_english() {
        assert_eq!(t(Lang::En, "never_label"), "never");
    }
}

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
    m.insert("register", ("Registracia", "Register"));
    m.insert("logout", ("Odhlasit", "Logout"));
    m.insert("my_bookings", ("Moje rezervacie", "My Bookings"));
    m.insert("balance", ("Zostatok", "Balance"));
    m.insert("staff", ("Personal", "Staff"));
    m.insert("admin", ("Administracia", "Admin"));
    m.insert("cards", ("Karty", "Cards"));
    m.insert("payments", ("Platby", "Payments"));
    m.insert("classes", ("Hodiny", "Classes"));

    // Auth forms
    m.insert("email", ("Email", "Email"));
    m.insert("password", ("Heslo", "Password"));
    m.insert("name", ("Meno", "Name"));
    m.insert("phone", ("Telefon", "Phone"));
    m.insert(
        "phone_optional",
        ("Telefon (volitelne)", "Phone (optional)"),
    );
    m.insert("first_name", ("Meno", "First Name"));
    m.insert("last_name", ("Priezvisko", "Last Name"));
    m.insert("company", ("Firma", "Company"));
    m.insert("cardholder", ("Drzitel karty", "Cardholder"));
    m.insert(
        "transaction_history",
        ("Historia transakcii", "Transaction History"),
    );
    m.insert(
        "no_transactions_card",
        ("Ziadne transakcie", "No transactions"),
    );
    m.insert("date", ("Datum", "Date"));
    m.insert("action", ("Akcia", "Action"));
    m.insert("logging_in", ("Prihlasovanie...", "Logging in..."));
    m.insert(
        "creating_account",
        ("Vytvaram ucet...", "Creating account..."),
    );
    m.insert(
        "dont_have_account",
        ("Nemate ucet? ", "Don't have an account? "),
    );
    m.insert(
        "already_have_account",
        ("Uz mate ucet? ", "Already have an account? "),
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
    m.insert("thu", ("St", "Thu"));
    m.insert("fri", ("Pi", "Fri"));
    m.insert("sat", ("So", "Sat"));
    m.insert("sun", ("Ne", "Sun"));

    // Day names (long)
    m.insert("monday", ("Pondelok", "Monday"));
    m.insert("tuesday", ("Utorok", "Tuesday"));
    m.insert("wednesday", ("Streda", "Wednesday"));
    m.insert("thursday", ("Stvrtok", "Thursday"));
    m.insert("friday", ("Piatok", "Friday"));
    m.insert("saturday", ("Sobota", "Saturday"));
    m.insert("sunday", ("Nedela", "Sunday"));

    // My bookings
    m.insert(
        "no_bookings",
        ("Ziadne nadchadzajuce rezervacie", "No upcoming bookings"),
    );

    // My balance
    m.insert("my_balance", ("Moj zostatok", "My Balance"));
    m.insert(
        "no_card_linked",
        ("Ziadna prepojena karta.", "No card linked to your account."),
    );
    m.insert("link_a_card", ("Prepojit kartu", "Link a Card"));
    m.insert("transactions", ("Transakcie", "Transactions"));
    m.insert(
        "no_transactions",
        ("Zatial ziadne transakcie.", "No transactions yet."),
    );

    // Link card
    m.insert("link_card", ("Prepojit kartu", "Link Card"));
    m.insert("card_barcode", ("Ciarovy kod karty", "Card Barcode"));
    m.insert(
        "scan_or_enter",
        (
            "Naskenujte alebo zadajte ciarovy kod",
            "Scan or enter barcode",
        ),
    );
    m.insert("linking", ("Prepajam...", "Linking..."));

    // Card operations
    m.insert("card_operations", ("Operacie s kartami", "Card Operations"));
    m.insert("barcode_lookup", ("Vyhladanie karty", "Barcode Lookup"));
    m.insert("enter_barcode", ("Zadajte ciarovy kod", "Enter barcode"));
    m.insert("lookup", ("Vyhladat", "Lookup"));
    m.insert(
        "activate_new_card",
        ("Aktivovat novu kartu", "Activate New Card"),
    );
    m.insert("activate", ("Aktivovat", "Activate"));
    m.insert(
        "all_member_cards",
        ("Vsetky clenske karty", "All Member Cards"),
    );
    m.insert("credit", ("Kredit", "Credit"));
    m.insert("status", ("Stav", "Status"));
    m.insert("linked", ("Prepojena", "Linked"));
    m.insert("yes", ("Ano", "Yes"));
    m.insert("no", ("Nie", "No"));
    m.insert("active", ("Aktivna", "Active"));
    m.insert("inactive", ("Neaktivna", "Inactive"));
    m.insert("blocked", ("Zablokovana", "Blocked"));
    m.insert("topup", ("Dobit", "Top Up"));
    m.insert("block", ("Zablokovat", "Block"));
    m.insert("unblock", ("Odblokovat", "Unblock"));
    m.insert("barcode", ("Ciarovy kod", "Barcode"));
    m.insert("initial_credit", ("Pociatocny kredit", "Initial Credit"));
    m.insert("no_cards_found", ("Ziadne karty", "No cards found"));
    m.insert("loading_cards", ("Nacitavam karty...", "Loading cards..."));
    m.insert(
        "new_card_barcode",
        ("Ciarovy kod novej karty", "New card barcode"),
    );

    // Card dashboard (fast search + actions)
    m.insert(
        "card_dashboard",
        ("Karty — rychly prehlad", "Cards — Quick Dashboard"),
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
    m.insert("quick_topup", ("Rychle dobitie", "Quick top-up"));
    m.insert("quick_charge", ("Rychla platba", "Quick charge"));
    m.insert("custom_amount", ("Vlastna suma", "Custom amount"));
    m.insert("hide_activate", ("Skryt aktivaciu", "Hide activation"));
    m.insert(
        "topup_ok_format",
        ("Dobite! Novy kredit: {} €", "Topped up! New credit: {} €"),
    );
    m.insert(
        "charge_ok_format",
        ("Uctovane. Zostatok: {} €", "Charged. Balance: {} €"),
    );
    m.insert("block_ok", ("Karta zablokovana", "Card blocked"));
    m.insert("unblock_ok", ("Karta odblokovana", "Card unblocked"));
    m.insert("activate_ok", ("Karta aktivovana", "Card activated"));
    m.insert("saved", ("Ulozene", "Saved"));

    // Staff dashboard
    m.insert("staff_dashboard", ("Panel personalu", "Staff Dashboard"));
    m.insert("walk_in", ("Vstup bez rezervacie", "Walk-in"));
    m.insert("add_walk_in", ("+ Navstevnik", "+ Walk-in"));
    m.insert("user_id", ("ID pouzivatela", "User ID"));
    m.insert("participants", ("Ucastnici", "Participants"));
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
    m.insert(
        "card_has_no_user",
        (
            "Karta nema priradeneho pouzivatela",
            "Card has no linked user",
        ),
    );

    // Payments
    m.insert("card_barcode_label", ("Ciarovy kod karty", "Card Barcode"));
    m.insert("scan_barcode", ("Naskenujte ciarovy kod", "Scan barcode"));
    m.insert("charge", ("Platba", "Charge"));
    m.insert("storno", ("Storno", "Storno"));
    m.insert("storno_refund", ("Storno (Vratenie)", "Storno (Refund)"));
    m.insert("service", ("Sluzba", "Service"));
    m.insert("select_service", ("-- Vyberte --", "-- Select --"));
    m.insert("amount", ("Suma", "Amount"));
    m.insert("amount_czk", ("Suma (EUR)", "Amount (EUR)"));
    m.insert("price", ("Cena", "Price"));
    m.insert("price_czk", ("Cena (EUR)", "Price (EUR)"));

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

    // Monthly pass banner
    m.insert(
        "pass_valid_until",
        ("✓ Mesacny listok platny do", "✓ Monthly pass valid until"),
    );
    m.insert(
        "pass_days_remaining",
        (
            "dni zostava · neobmedzeny pristup",
            "days remaining · unlimited access",
        ),
    );
    m.insert(
        "pass_expired",
        ("Mesacny listok expiroval pred", "Monthly pass expired"),
    );
    m.insert("pass_days_ago", ("dnami", "days ago"));
    m.insert(
        "pass_last_valid_until",
        ("Naposledy platny do", "Last valid until"),
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

    // Transaction history action labels (DB stores raw English: topup/charge/visit).
    m.insert("tx_action_topup", ("Dobitie", "Top-up"));
    m.insert("tx_action_charge", ("Platba", "Charge"));
    m.insert("tx_action_visit", ("Navsteva", "Visit"));

    // Sell pass modal
    m.insert(
        "sell_monthly_pass",
        ("Predat mesacny listok", "Sell monthly pass"),
    );
    m.insert("modal_price", ("Cena (EUR)", "Price (EUR)"));
    m.insert("modal_valid_until", ("Platny do", "Valid until"));
    m.insert("modal_confirm", ("Predat", "Sell pass"));
    m.insert("modal_cancel", ("Zrusit", "Cancel"));

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

    // General
    m.insert("loading", ("Nacitavanie...", "Loading..."));
    m.insert("page_not_found", ("Stranka nenajdena", "Page not found"));
    m.insert(
        "unable_to_load",
        ("Nepodarilo sa nacitat", "Unable to load"),
    );

    // Redesign 2026: new UI strings
    m.insert("show_older", ("Zobrazit starsie", "Show older"));
    m.insert("close", ("Zatvorit", "Close"));
    m.insert("edit_info", ("Upravit udaje", "Edit info"));
    m.insert("customer_info", ("Udaje klienta", "Customer info"));
    m.insert(
        "sell_pass_label",
        ("Predat mesacny preukaz", "Sell monthly pass"),
    );
    m.insert("pass_active_until", ("Aktivny do {}", "Active until {}"));
    m.insert("pass_expired_on", ("Skoncil {}", "Expired {}"));
    m.insert("days_left_short", ("{} d", "{}d"));
    m.insert("days_ago_short", ("pred {} d", "{}d ago"));
    m.insert("edit_pass_date", ("Upravit datum", "Edit date"));

    m
});

fn translations() -> &'static TransMap {
    &TRANSLATIONS
}

/// Get the short day name keys in order Mon-Sun
pub static DAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Admin tab name translations
pub static ADMIN_TAB_KEYS: [(&str, &str); 5] = [
    ("templates", "templates"),
    ("instructors", "instructors"),
    ("services", "services"),
    ("users", "users"),
    ("settings", "settings"),
];

/// Weekday names used in admin (short) - same as DAY_KEYS
pub static WEEKDAY_KEYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

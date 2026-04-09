use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ClientMsg {
    Ping,
    SubscribeSchedule { date: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerMsg {
    BookingUpdate {
        template_id: i64,
        date: String,
        booked: i32,
        capacity: i32,
    },
    ClassCancelled {
        template_id: i64,
        date: String,
    },
    Pong,
}

#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use std::time::SystemTime;

use rocket::State;
use rocket_contrib::json::{Json, JsonValue};
use priority_queue::PriorityQueue;

type Priority = usize;
type Timestamp = u128;
// This defines the format of the message we receive.
#[derive(Serialize, Deserialize)]
struct MessageIn {
    contents: String,
    priority: Option<Priority>,
}
// This defines the format of the message we track internally. (The
// priority is tracked in the PriorityQueue, no need to duplicate.)
#[derive(PartialEq, Eq, Hash)]
struct MessageInternal {
    contents: String,
    arrived: Timestamp,
}
// This defines the format of the message we push downstream.
#[derive(Serialize, Deserialize)]
struct MessageOut {
    contents: String,
    priority: Priority,
    elapsed: usize,
}
type MessageQueue = Mutex<PriorityQueue<MessageInternal, Priority>>;

// Helper function for getting time since the epoch.
fn time_since_epoch() -> Timestamp {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_millis(),
        Err(_) => 0,
    }
}

// Handle POSTs to the proxy.
#[post("/", format="json", data="<message>")]
fn new(message: Json<MessageIn>, queue: State<'_, MessageQueue>) -> JsonValue {
    // Priority is optional, set a default if not provided.
    let prio: Priority;
    match message.0.priority {
        None => {
            prio = 10;
        }
        _ => {
            prio = message.0.priority.unwrap();
        }
    }
    let mut messagequeue = queue.lock().expect("queue lock.");
    let internal = MessageInternal {
        contents: message.0.contents,
        arrived: time_since_epoch(),
    };
    messagequeue.push(internal, prio);
    json!({
        "status": "ok",
        "code": 200,
    })
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(queue: State<'_, MessageQueue>) -> Option<Json<MessageOut>> {
    let mut messagequeue = queue.lock().unwrap();
    messagequeue.pop().map(|internal| {
        Json(MessageOut {
            contents: internal.0.contents.clone(),
            priority: internal.1,
            elapsed: (time_since_epoch() - internal.0.arrived) as usize,
        })
    })
}

#[catch(404)]
fn not_found() -> JsonValue {
    json!({
        "status": "error",
        "code": 404,
        "reason": "Resource was not found.",
    })
}

fn rocket() -> rocket::Rocket {
    rocket::ignite()
        .mount("/", routes![new, get])
        .register(catchers![not_found])
        .manage(Mutex::new(PriorityQueue::<MessageInternal, Priority>::new()))
}

fn main() {
    rocket().launch();
}

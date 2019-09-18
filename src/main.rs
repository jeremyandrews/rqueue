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
use rocket::http::{ContentType, Status};
use rocket::response;
use rocket::request;

type Priority = usize;
type Timestamp = u128;
// This defines the format of the message we receive.
#[derive(Serialize, Deserialize)]
struct MessageIn {
    contents: String,
    priority: Option<Priority>,
}
// This defines the format of the message we track internally.
#[derive(PartialEq, Eq, Hash)]
struct MessageInternal {
    contents: String,
    priority: Priority,
    arrived: Timestamp,
}
type MessageQueue = Mutex<PriorityQueue<MessageInternal, Priority>>;
// Set an HTTP status when responding with JSON objects
#[derive(Debug)]
struct QueueApiResponse {
    json: JsonValue,
    status: Status,
}

// Helper function for getting time since the epoch in milliseconds.
fn time_since_epoch() -> Timestamp {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_millis(),
        Err(_) => 0,
    }
}

// Customer JSON responder, includes an HTTP status message.
impl<'r> response::Responder<'r> for QueueApiResponse {
    fn respond_to(self, req: &request::Request) -> response::Result<'r> {
        response::Response::build_from(self.json.respond_to(&req).unwrap())
            .status(self.status)
            .header(ContentType::JSON)
            .ok()
    }
}

// Accept incoming messages for the proxy to queue.
#[post("/", format="json", data="<message>")]
fn new(message: Json<MessageIn>, queue: State<'_, MessageQueue>) -> QueueApiResponse {
    // Priority is optional, set a default if not provided.
    let priority: Priority;
    match message.0.priority {
        None => {
            priority = 10;
        }
        _ => {
            priority = message.0.priority.unwrap();
        }
    }
    // Internal state, the queue 
    let internal = MessageInternal {
        contents: message.0.contents,
        priority: priority,
        arrived: time_since_epoch(),
    };

    // Grab lock and add message to queue
    let mut messagequeue = queue.lock().expect("queue lock.");
    messagequeue.push(internal, priority);
    QueueApiResponse {
        json: json!({
                "status": "accepted",
                "code": 202,
            }),
        status: Status::Accepted,
    }
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(queue: State<'_, MessageQueue>) -> Option<QueueApiResponse> {
    let mut messagequeue = queue.lock().unwrap();
    messagequeue.pop().map(|internal| {
        // Message queue returns a tuple, the internal data strucutre and the priority.
        // Use this to build the JSON response on-the-fly.
        QueueApiResponse {
            json: json!({
                    "status": "ok",
                    "code": 200,
                    "data": {
                        "contents": internal.0.contents.clone(),
                        "priority": internal.0.priority,
                        "elapsed": (time_since_epoch() - internal.0.arrived) as usize,
                    }
                }),
            status: Status::Ok,
        }
    })
}

#[catch(404)]
fn not_found() -> QueueApiResponse {
    QueueApiResponse {
        json: json!({
                "status": "error",
                "code": 404,
                "reason": "Resource was not found.",
            }),
        status: Status::NotFound,
    }
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

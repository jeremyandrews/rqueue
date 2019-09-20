#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use std::time::{SystemTime, Duration};
use std::sync::atomic::{AtomicUsize, Ordering};

use rocket::{State, Request, response};
use rocket::http::{ContentType, Status};
use rocket_contrib::json::{Json, JsonValue};


use priority_queue::PriorityQueue;

type Priority = u8;
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

// Global counters:
#[derive(Default)]
struct Counters {
    queue_requests: AtomicUsize,
    proxy_requests: AtomicUsize,
    queued: AtomicUsize,
    proxied: AtomicUsize,
    in_queue: AtomicUsize,
}

// Set an HTTP status when responding with JSON objects
#[derive(Debug)]
struct QueueApiResponse {
    json: JsonValue,
    status: Status,
}

#[derive(Debug)]
struct ServerStarted(Duration);

// Customer JSON responder, includes an HTTP status message.
impl<'r> response::Responder<'r> for QueueApiResponse {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        response::Response::build_from(self.json.respond_to(&req).unwrap())
            .status(self.status)
            .header(ContentType::JSON)
            .ok()
    }
}

// Helper function for getting time since the epoch in milliseconds.
fn time_since_epoch() -> Duration {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n,
        Err(_) => Duration::from_secs(0),
    }
}

// Helper function for getting elapsed time in milliseconds (as usize).
fn milliseconds_since_timestamp(timestamp: Duration) -> usize {
    let now = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_millis(),
        Err(_) => 0,
    };
    // Convert to usize for JSON compatability -- @TODO handle time going backwards
    (now - timestamp.as_millis()) as usize
}

// Accept incoming messages for the proxy to queue.
#[post("/", format="json", data="<message>")]
fn new(message: Json<MessageIn>, queue: State<'_, MessageQueue>, counters: State<Counters>, server_started: State<ServerStarted>) -> QueueApiResponse {
    // A POST was routed here, requesting to add something to the queue.
    let queue_requests = counters.queue_requests.fetch_add(1, Ordering::Relaxed) + 1;

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
        arrived: time_since_epoch().as_millis(),
    };

    // Grab lock and add message to queue
    let mut messagequeue = queue.lock().expect("queue lock.");
    messagequeue.push(internal, priority);
    // A message has been sucessfully added to the queue.
    let queued = counters.queued.fetch_add(1, Ordering::Relaxed) + 1;
    let in_queue = counters.in_queue.fetch_add(1, Ordering::Relaxed) + 1;
    // Retreive other debug statistics
    let proxy_requests = counters.proxy_requests.load(Ordering::Relaxed);
    let proxied = counters.proxied.load(Ordering::Relaxed);
    QueueApiResponse {
        json: json!({
                "status": "accepted",
                "code": 202,
                "debug": {
                    "queue_requests": queue_requests,
                    "proxy_requests": proxy_requests,
                    "queued": queued,
                    "proxied": proxied,
                    "in_queue": in_queue,
                    "uptime": milliseconds_since_timestamp(server_started.0),
                },
            }),
        status: Status::Accepted,
    }
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(queue: State<'_, MessageQueue>, counters: State<Counters>, server_started: State<ServerStarted>) -> Option<QueueApiResponse> {
    // A GET was routed here, requesting to get something from the queue.
    let proxy_requests = counters.proxy_requests.fetch_add(1, Ordering::Relaxed) + 1;

    let mut messagequeue = queue.lock().unwrap();
    messagequeue.pop().map(|internal| {
        // A message has been sucessfully removed from the queue.
        let proxied = counters.proxied.fetch_add(1, Ordering::Relaxed) + 1;
        let in_queue = counters.in_queue.fetch_sub(1, Ordering::Relaxed) - 1;
        // Retreive other debug statistics
        let queue_requests = counters.queue_requests.load(Ordering::Relaxed);
        let queued = counters.queued.load(Ordering::Relaxed);

        // Message queue returns a tuple, the internal data strucutre and the priority.
        // Use this to build the JSON response on-the-fly.
        QueueApiResponse {
            json: json!({
                    "status": "ok",
                    "code": 200,
                    "data": {
                        "contents": internal.0.contents.clone(),
                        "priority": internal.0.priority,
                        "elapsed": (time_since_epoch().as_millis() - internal.0.arrived) as usize,
                    },
                    "debug": {
                        "queue_requests": queue_requests,
                        "proxy_requests": proxy_requests,
                        "queued": queued,
                        "proxied": proxied,
                        "in_queue": in_queue,
                        "uptime": milliseconds_since_timestamp(server_started.0),
                    },
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
                "reason": "Empty queue.",
            }),
        status: Status::NotFound,
    }
}

fn rocket() -> rocket::Rocket {
    rocket::ignite()
        .register(catchers![not_found])
        .manage(Counters::default())
        .manage(Mutex::new(PriorityQueue::<MessageInternal, Priority>::new()))
        .manage(ServerStarted(time_since_epoch()))
        .mount("/", routes![new, get])
}

fn main() {
    rocket().launch();
}

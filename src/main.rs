#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use std::time::{SystemTime, Duration};
use std::sync::atomic::{AtomicUsize, Ordering};

use rocket::{State, Request, response};
use rocket::fairing::AdHoc;
use rocket::http::{ContentType, Status};
use rocket::outcome::Outcome;
use rocket::request::{self, FromRequest};
use rocket_contrib::json::{Json, JsonValue};

use priority_queue::PriorityQueue;
use uuid::Uuid;
use sha2::{Sha256, Digest};

type Priority = u8;
type Timestamp = u128;
// This defines the format of the message we receive.
#[derive(Serialize, Deserialize)]
struct MessageIn {
    contents: String,
    sha256: Option<String>,
    priority: Option<Priority>,
}
// This defines the format of the message we track internally.
#[derive(PartialEq, Eq, Hash)]
struct MessageInternal {
    contents: String,
    sha256: String,
    priority: Priority,
    arrived: Timestamp,
    uuid: Uuid,
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

#[derive(Clone, Debug)]
struct Started(Duration);

// Set an HTTP status when responding with JSON objects
#[derive(Debug)]
struct QueueApiResponse {
    json: JsonValue,
    status: Status,
}

// Customer JSON responder, includes an HTTP status message.
impl<'r> response::Responder<'r> for QueueApiResponse {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        response::Response::build_from(self.json.respond_to(&req).unwrap())
            .status(self.status)
            .header(ContentType::JSON)
            .ok()
    }
}

impl<'a, 'r> FromRequest<'a, 'r> for Started {
    type Error = std::convert::Infallible;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let request_started: &Started = request.local_cache(|| Started(Duration::from_secs(0)));
        Outcome::Success(request_started.clone())
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
fn new(
        message: Json<MessageIn>,
        queue: State<'_, MessageQueue>,
        counters: State<Counters>,
        server_started: State<Started>,
        request_started: Started,
    ) -> QueueApiResponse {
    // A POST was routed here, requesting to add something to the queue.
    let queue_requests = counters.queue_requests.fetch_add(1, Ordering::Relaxed) + 1;

    // Generate a Sha256 of the message contents.
    let mut hasher = Sha256::new();
    hasher.input(message.0.contents.as_bytes());
    let sha256 = format!("{:x}", hasher.result());

    // If a Sha256 was provided, validate it
    match message.0.sha256 {
        None => {
            // The Sha256 is not required.
        },
        _ => {
            let sha256_received = message.0.sha256.unwrap();
            if sha256 != sha256_received.to_lowercase() {
                return QueueApiResponse {
                    json: json!({
                            "status": "invalid sha256",
                            "code": 400,
                            "debug": {
                                "expected_sha256": sha256,
                                "received_sha256": sha256_received,
                            },
                        }),
                    status: Status::BadRequest,
                };
            }
        },
    }

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
        sha256: sha256,
        priority: priority,
        arrived: time_since_epoch().as_millis(),
        uuid: Uuid::new_v4(),
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
                    "process_time": milliseconds_since_timestamp(request_started.0),
                },
            }),
        status: Status::Accepted,
    }
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(
        queue: State<'_, MessageQueue>,
        counters: State<Counters>,
        server_started: State<Started>,
        request_started: Started,
    ) -> Option<QueueApiResponse> {
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
                        "sha256": internal.0.sha256.clone(),
                        "priority": internal.0.priority,
                        "elapsed": (time_since_epoch().as_millis() - internal.0.arrived) as usize,
                        "uuid": internal.0.uuid,
                    },
                    "debug": {
                        "queue_requests": queue_requests,
                        "proxy_requests": proxy_requests,
                        "queued": queued,
                        "proxied": proxied,
                        "in_queue": in_queue,
                        "uptime": milliseconds_since_timestamp(server_started.0),
                        "process_time": milliseconds_since_timestamp(request_started.0),
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
        .manage(Started(time_since_epoch()))
        .attach(AdHoc::on_request("Time Request", |req, _| {
            req.local_cache(|| Started(time_since_epoch()));
        }))
        .mount("/", routes![new, get])
}

fn main() {
    rocket().launch();
}

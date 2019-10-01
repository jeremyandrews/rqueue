#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate lazy_static;

#[cfg(test)] mod tests;

use std::sync::{Mutex, Arc};
use std::time::{SystemTime, Duration};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use rocket::{State, Request, response};
use rocket::fairing::AdHoc;
use rocket::http::{ContentType, Status};
use rocket::outcome::Outcome;
use rocket::request::{self, FromRequest};
use rocket_contrib::json::{Json, JsonValue};

use priority_queue::PriorityQueue;
use uuid::Uuid;
use sha2::{Sha256, Digest};
use size::{Base, Size, Style};

mod proxy;

type Priority = u8;
type Timestamp = u128;
type SizeInBytes = AtomicUsize;

// By default limit queue size to ~64 MiB
const DEFAULT_MAXIMUM_QUEUE_SIZE: usize = 1024 * 1024 * 64;
// Default priority to 10 if not otherwise set
const DEFAULT_PRIORITY: u8 = 10;
// By default wait 5 seconds after checking an empty queue
const DEFAULT_PROXY_DELAY: usize = 5;

// This defines the format of the message we receive.
#[derive(Serialize, Deserialize)]
struct MessageIn {
    contents: String,
    sha256: Option<String>,
    priority: Option<i32>,
}
// This defines the format of the message we track internally.
#[derive(PartialEq, Eq, Hash, Debug, Serialize, Deserialize, Default)]
struct MessageInternal {
    size_in_bytes: usize,
    contents: String,
    sha256: String,
    priority: Priority,
    arrived: Timestamp,
    uuid: Uuid,
}

// Global counters:
#[derive(Default)]
struct Counters {
    queue_requests: AtomicUsize,
    proxy_requests: AtomicUsize,
    queued: AtomicUsize,
    proxied: AtomicUsize,
    in_queue: AtomicUsize,
    bytes: AtomicUsize,
}

// Proxy configuration:
#[derive(Default)]
struct ProxyConfig {
    delay: usize,
    server: String,
}

#[derive(Clone, Debug)]
pub struct RequestTimer(Duration);
#[derive(Clone, Debug)]
struct QueueMemoryLimit(usize);

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

impl<'a, 'r> FromRequest<'a, 'r> for RequestTimer {
    type Error = std::convert::Infallible;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        let request_started: &RequestTimer = request.local_cache(|| RequestTimer(Duration::from_secs(0)));
        Outcome::Success(request_started.clone())
    }
}

lazy_static! {
    static ref STARTED_1: Arc<Mutex<Duration>> = Arc::new(Mutex::new(time_since_epoch()));
    static ref COUNTERS: Arc<Mutex<Counters>> = Arc::new(Mutex::new(Counters::default()));
    static ref QUEUE: Arc<Mutex<PriorityQueue<MessageInternal, Priority>>> = Arc::new(Mutex::new(PriorityQueue::<MessageInternal, Priority>::new()));
    static ref PROXY_CONFIG: Arc<Mutex<ProxyConfig>> = Arc::new(Mutex::new(ProxyConfig::default()));
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
        request_started: RequestTimer,
        queue_memory_limit: State<QueueMemoryLimit>,
    ) -> QueueApiResponse {
    let server_started = STARTED_1.lock().unwrap();
    let counters = COUNTERS.lock().unwrap();
    // A POST was routed here, requesting to add something to the queue.
    let queue_requests = counters.queue_requests.fetch_add(1, Ordering::Relaxed) + 1;

    // Generate a Sha256 of the message contents.
    let mut hasher = Sha256::new();
    hasher.input(message.0.contents.as_bytes());
    let sha256 = format!("{:x}", hasher.result());
    log::debug!("{}|generated sha256{} for message '{}'",
        milliseconds_since_timestamp(*server_started),
        sha256,
        message.0.contents,
    );

    // If a Sha256 was provided, validate it
    match message.0.sha256 {
        None => {
            // The Sha256 is not required.
        },
        _ => {
            let sha256_received = message.0.sha256.unwrap();
            if sha256 != sha256_received.to_lowercase() {
                log::info!("{}|invalid sha256 {} received, expected {}, ignoring message",
                    milliseconds_since_timestamp(*server_started),
                    sha256_received,
                    sha256,
                );
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid sha256",
                            "code": 400,
                            "debug": {
                                "uptime": milliseconds_since_timestamp(*server_started),
                                "process_time": milliseconds_since_timestamp(request_started.0),
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
            priority = DEFAULT_PRIORITY;
            log::debug!("{}|automatically set priority to {}",
                milliseconds_since_timestamp(*server_started),
                priority,
            );
        }
        _ => {
            let temporary_priority: i32 = message.0.priority.unwrap();
            if temporary_priority < u8::min_value() as i32 {
                log::info!("{}|received invalid negative priority of {}",
                    milliseconds_since_timestamp(*server_started),
                    temporary_priority,
                );
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid priority",
                            "code": 400,
                            "debug": {
                                "uptime": milliseconds_since_timestamp(*server_started),
                                "process_time": milliseconds_since_timestamp(request_started.0),
                                "minimum_priority": Priority::min_value(),
                                "received_priority": temporary_priority,
                            },
                        }),
                    status: Status::BadRequest,
                };
            }
            else if temporary_priority > u8::max_value() as i32 {
                log::info!("{}|received invalid priority of {}",
                    milliseconds_since_timestamp(*server_started),
                    temporary_priority,
                );
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid priority",
                            "code": 400,
                            "debug": {
                                "uptime": milliseconds_since_timestamp(*server_started),
                                "process_time": milliseconds_since_timestamp(request_started.0),
                                "maximum_priority": Priority::max_value(),
                                "received_priority": temporary_priority,
                            },
                        }),
                    status: Status::BadRequest,
                };
            }
            else {
                priority = temporary_priority as u8;
                log::debug!("{}|manually set priority to {}",
                    milliseconds_since_timestamp(*server_started),
                    priority,
                );
            }
        }
    }
    // Internal state, the queue 
    let internal = MessageInternal {
        // Size required is the size of this struct, plus the capacity of both contained strings
        size_in_bytes: std::mem::size_of::<MessageInternal>() + message.0.contents.capacity() + sha256.capacity(),
        contents: message.0.contents,
        sha256: sha256,
        priority: priority,
        arrived: time_since_epoch().as_millis(),
        uuid: Uuid::new_v4(),
    };
    let bytes_allocated_for_queue = counters.bytes.load(Ordering::Relaxed);
    if (bytes_allocated_for_queue + internal.size_in_bytes) > queue_memory_limit.0 {
        log::warn!("{}|queue is holding {}, limit of {}, unable to store additional {}",
            milliseconds_since_timestamp(*server_started),
            Size::Bytes(bytes_allocated_for_queue),
            Size::Bytes(queue_memory_limit.0),
            Size::Bytes(internal.size_in_bytes)
        );
        return QueueApiResponse {
            json: json!({
                    "status": "service unavailable",
                    "reason": "insufficient memory",
                    "code": 503,
                    "debug": {
                        "uptime": milliseconds_since_timestamp(*server_started),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
                        "request_size": format!("{}", Size::Bytes(internal.size_in_bytes)),
                        "max_bytes": format!("{}", Size::Bytes(queue_memory_limit.0)),
                    },
                }),
            status: Status::ServiceUnavailable,
        };
    }

    // Clone this so we can increment bytes_allocated_for_queue
    let size_of_request = internal.size_in_bytes.clone();

    // Grab lock and add message to queue
    let mut queue = QUEUE.lock().expect("queue lock");
    queue.push(internal, priority);

    // A message has been sucessfully added to the queue.
    let queued = counters.queued.fetch_add(1, Ordering::Relaxed) + 1;
    let in_queue = counters.in_queue.fetch_add(1, Ordering::Relaxed) + 1;
    let bytes_allocated_for_queue = counters.bytes.fetch_add(size_of_request, Ordering::Relaxed) + size_of_request;
    // Retreive other debug statistics
    let proxy_requests = counters.proxy_requests.load(Ordering::Relaxed);
    let proxied = counters.proxied.load(Ordering::Relaxed);

    log::info!("{}|{} message with priority of {} queued, {} queue_requests, {} queued, {} proxy requests, {} proxied, {} in {} queue, request took {} ms",
        milliseconds_since_timestamp(*server_started),
        Size::Bytes(size_of_request).to_string(Base::Base10, Style::Abbreviated),
        priority,
        queue_requests,
        queued,
        proxy_requests,
        proxied,
        in_queue,
        Size::Bytes(bytes_allocated_for_queue).to_string(Base::Base10, Style::Abbreviated),
        milliseconds_since_timestamp(request_started.0),
    );

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
                    "uptime": milliseconds_since_timestamp(*server_started),
                    "process_time": milliseconds_since_timestamp(request_started.0),
                    "request_size": format!("{}", Size::Bytes(size_of_request)),
                    "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
                },
            }),
        status: Status::Accepted,
    }
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(
        request_started: RequestTimer,
    ) -> Option<QueueApiResponse> {
    let server_started = STARTED_1.lock().unwrap();
    let counters = COUNTERS.lock().unwrap();
    // A GET was routed here, requesting to get something from the queue.
    let proxy_requests = counters.proxy_requests.fetch_add(1, Ordering::Relaxed) + 1;

    let mut queue = QUEUE.lock().expect("queue lock");
    queue.pop().map(|internal| {
        // A message has been sucessfully removed from the queue.
        let proxied = counters.proxied.fetch_add(1, Ordering::Relaxed) + 1;
        let in_queue = counters.in_queue.fetch_sub(1, Ordering::Relaxed) - 1;
        let bytes_allocated_for_queue = counters.bytes.fetch_sub(internal.0.size_in_bytes, Ordering::Relaxed) - internal.0.size_in_bytes;
        // Retreive other debug statistics
        let queue_requests = counters.queue_requests.load(Ordering::Relaxed);
        let queued = counters.queued.load(Ordering::Relaxed);

        log::debug!("{}|message from queue with sha256 {}: '{}'",
            milliseconds_since_timestamp(*server_started),
            internal.0.sha256,
            internal.0.contents,
        );
        log::info!("{}|{} message with priority of {} proxied, {} queue_requests, {} queued, {} proxy requests, {} proxied, {} in {} queue, request took {} ms",
            milliseconds_since_timestamp(*server_started),
            Size::Bytes(internal.0.size_in_bytes).to_string(Base::Base10, Style::Abbreviated),
            internal.0.priority,
            queue_requests,
            queued,
            proxy_requests,
            proxied,
            in_queue,
            Size::Bytes(bytes_allocated_for_queue).to_string(Base::Base10, Style::Abbreviated),
            milliseconds_since_timestamp(request_started.0),
        );

        // Message queue returns a tuple, the internal data strucutre and the priority.
        // Use this to build the JSON response on-the-fly.
        QueueApiResponse {
            json: json!({
                    "status": "ok",
                    "code": 200,
                    "data": {
                        "contents": internal.0.contents,
                        "sha256": internal.0.sha256,
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
                        "uptime": milliseconds_since_timestamp(*server_started),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
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
        .attach(AdHoc::on_attach("Custom Configuration", |rocket| {
            let memory_limit_config = rocket.config()
                .get_int("queue_memory_limit_in_bytes");
            
            let queue_memory_limit = match memory_limit_config {
                Ok(n) => n as usize,
                Err(_) => DEFAULT_MAXIMUM_QUEUE_SIZE,
            };
            log::info!("Queue memory limit: {}", Size::Bytes(queue_memory_limit));

            let mut proxy_config = PROXY_CONFIG.lock().unwrap();

            let proxy_delay_config = rocket.config()
                .get_int("proxy_delay");
            proxy_config.delay = match proxy_delay_config {
                Ok(n) => {
                    if n > 0 {
                        n as usize
                    }
                    else {
                        5
                    }
                }
                Err(_) => DEFAULT_PROXY_DELAY,
            };
            log::info!("Proxy delay: {} s", proxy_config.delay);

            let proxy_notification_server = rocket.config()
                .get_str("notification_server");
            proxy_config.server = match proxy_notification_server {
                Ok(n) => n.to_string(),
                Err(_) => "None".to_string(),
            };
            log::info!("Notification server: {}", proxy_config.server);

            Ok(rocket.manage(QueueMemoryLimit(queue_memory_limit)))
        }))
        .attach(AdHoc::on_request("Time Request", |req, _| {
            req.local_cache(|| RequestTimer(time_since_epoch()));
        }))
        .register(catchers![not_found])
        .mount("/", routes![new, get])
}

fn main() {
    // Proxy thread reads queue and pushes notifications upstream.
    thread::spawn(|| {
        proxy::proxy_loop();
    });
    // REST server collects notifications in the queue.
    rocket().launch();
}

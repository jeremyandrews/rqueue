#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
#[macro_use] extern crate lazy_static;

#[cfg(test)] mod tests;
//#[cfg(feature = "rqueue-proxy")] mod proxy;
//#[cfg(feature = "rqueue-notify")] mod notify;
mod proxy;
mod notify;

use std::sync::{Mutex, Arc};
use std::time::{SystemTime, Duration};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::process;

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

type Priority = u8;
type Timestamp = u128;
type SizeInBytes = AtomicUsize;

// By default limit queue size to ~64 MiB
const DEFAULT_MAXIMUM_QUEUE_SIZE: usize = 1024 * 1024 * 64;
// Default priority to 10 if not otherwise set
const DEFAULT_PRIORITY: u8 = 10;
// By default wait 5 seconds after checking an empty queue
const DEFAULT_DELAY: usize = 5;

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
    delivery_attempts: usize,
    original_priority: Priority,
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

// Queue configuration:
#[derive(Default)]
struct QueueConfig {
    memory_limit: usize,
    require_sha256: bool,
    shared_secret: String,
}
// Proxy configuration:
#[derive(Default)]
struct ProxyConfig {
    delay: usize,
    server: String,
}
// Notify configuration:
#[derive(Default)]
struct NotifyConfig {
    delay: usize,
    mail_from_name: String,
    mail_from_address: String,
    mail_to_name: String,
    mail_to_address: String,
    smtp_server: String,
    smtp_user: String,
    smtp_password: String,
}

#[derive(Clone, Debug)]
struct Started(Duration);
#[derive(Clone, Debug)]
pub struct RequestTimer(Duration);

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
    static ref COUNTERS: Arc<Mutex<Counters>> = Arc::new(Mutex::new(Counters::default()));
    static ref QUEUE: Arc<Mutex<PriorityQueue<MessageInternal, Priority>>> = Arc::new(Mutex::new(PriorityQueue::<MessageInternal, Priority>::new()));
    static ref PROXY_CONFIG: Arc<Mutex<ProxyConfig>> = Arc::new(Mutex::new(ProxyConfig::default()));
    static ref NOTIFY_CONFIG: Arc<Mutex<NotifyConfig>> = Arc::new(Mutex::new(NotifyConfig::default()));
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
        server_started: State<Started>,
        request_started: RequestTimer,
        queue_config: State<QueueConfig>,
    ) -> QueueApiResponse {
    let counters = COUNTERS.lock().unwrap();
    // A POST was routed here, requesting to add something to the queue.
    let queue_requests = counters.queue_requests.fetch_add(1, Ordering::Relaxed) + 1;

    // Generate a Sha256 of the message contents.
    let mut hasher = Sha256::new();
    hasher.input(message.0.contents.as_bytes());
    if queue_config.shared_secret != "" {
        hasher.input(queue_config.shared_secret.as_bytes());
    }
    let sha256 = format!("{:x}", hasher.result());
    log::debug!("{}|generated sha256{} for message '{}'",
        milliseconds_since_timestamp(server_started.0),
        sha256,
        message.0.contents,
    );

    // If a Sha256 was provided, validate it
    match message.0.sha256 {
        None => {
            if queue_config.require_sha256 {
                log::warn!("{}|sha256 required but not set, expected {}, ignoring message",
                    milliseconds_since_timestamp(server_started.0),
                    sha256,
                );
                let debug;
                if cfg!(feature = "rqueue-debug") {
                    debug = json!({
                        "uptime": milliseconds_since_timestamp(server_started.0),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "expected_sha256": sha256,
                    })
                }
                else {
                    debug = json!({})
                }
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "required sha256 not set",
                            "code": 400,
                            "debug": debug,
                        }),
                    status: Status::BadRequest,
                };
            }
        },
        _ => {
            let sha256_received = message.0.sha256.unwrap();
            if sha256 != sha256_received.to_lowercase() {
                log::info!("{}|invalid sha256 {} received, expected {}, ignoring message",
                    milliseconds_since_timestamp(server_started.0),
                    sha256_received,
                    sha256,
                );
                let debug;
                if cfg!(feature = "rqueue-debug") {
                    debug = json!({
                        "uptime": milliseconds_since_timestamp(server_started.0),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "expected_sha256": sha256,
                        "received_sha256": sha256_received,
                    })
                }
                else {
                    debug = json!({})
                }
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid sha256",
                            "code": 400,
                            "debug": debug,
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
                milliseconds_since_timestamp(server_started.0),
                priority,
            );
        }
        _ => {
            let temporary_priority: i32 = message.0.priority.unwrap();
            if temporary_priority < u8::min_value() as i32 {
                log::info!("{}|received invalid negative priority of {}",
                    milliseconds_since_timestamp(server_started.0),
                    temporary_priority,
                );
                let debug;
                if cfg!(feature = "rqueue-debug") {
                    debug = json!({
                        "uptime": milliseconds_since_timestamp(server_started.0),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "minimum_priority": Priority::min_value(),
                        "received_priority": temporary_priority,
                    })
                }
                else {
                    debug = json!({})
                }
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid priority",
                            "code": 400,
                            "debug": debug,
                        }),
                    status: Status::BadRequest,
                };
            }
            else if temporary_priority > u8::max_value() as i32 {
                log::info!("{}|received invalid priority of {}",
                    milliseconds_since_timestamp(server_started.0),
                    temporary_priority,
                );
                let debug;
                if cfg!(feature = "rqueue-debug") {
                    debug = json!({
                        "uptime": milliseconds_since_timestamp(server_started.0),
                        "process_time": milliseconds_since_timestamp(request_started.0),
                        "maximum_priority": Priority::max_value(),
                        "received_priority": temporary_priority,
                    })
                }
                else {
                    debug = json!({})
                }
                return QueueApiResponse {
                    json: json!({
                            "status": "bad request",
                            "reason": "invalid priority",
                            "code": 400,
                            "debug": debug,
                        }),
                    status: Status::BadRequest,
                };
            }
            else {
                priority = temporary_priority as u8;
                log::debug!("{}|manually set priority to {}",
                    milliseconds_since_timestamp(server_started.0),
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
        delivery_attempts: 0,
        original_priority: priority,
    };
    let bytes_allocated_for_queue = counters.bytes.load(Ordering::Relaxed);
    if (bytes_allocated_for_queue + internal.size_in_bytes) > queue_config.memory_limit {
        log::warn!("{}|queue is holding {}, limit of {}, unable to store additional {}",
            milliseconds_since_timestamp(server_started.0),
            Size::Bytes(bytes_allocated_for_queue),
            Size::Bytes(queue_config.memory_limit),
            Size::Bytes(internal.size_in_bytes)
        );
        let debug;
        if cfg!(feature = "rqueue-debug") {
            debug = json!({
                "uptime": milliseconds_since_timestamp(server_started.0),
                "process_time": milliseconds_since_timestamp(request_started.0),
                "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
                "request_size": format!("{}", Size::Bytes(internal.size_in_bytes)),
                "max_bytes": format!("{}", Size::Bytes(queue_config.memory_limit)),
            })
        }
        else {
            debug = json!({})
        }
        return QueueApiResponse {
            json: json!({
                    "status": "service unavailable",
                    "reason": "insufficient memory",
                    "code": 503,
                    "debug": debug,
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
        milliseconds_since_timestamp(server_started.0),
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

    let debug;
    if cfg!(feature = "rqueue-debug") {
        debug = json!({
            "queue_requests": queue_requests,
            "proxy_requests": proxy_requests,
            "queued": queued,
            "proxied": proxied,
            "in_queue": in_queue,
            "uptime": milliseconds_since_timestamp(server_started.0),
            "process_time": milliseconds_since_timestamp(request_started.0),
            "request_size": format!("{}", Size::Bytes(size_of_request)),
            "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
        })
    }
    else {
        debug = json!({})
    }
    QueueApiResponse {
        json: json!({
                "status": "accepted",
                "code": 202,
                "debug": debug,
            }),
        status: Status::Accepted,
    }
}

// Temporary: ultimately the proxy will push this data.
#[get("/", format = "json")]
fn get(
        request_started: RequestTimer,
        server_started: State<Started>,
    ) -> Option<QueueApiResponse> {
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
            milliseconds_since_timestamp(server_started.0),
            internal.0.sha256,
            internal.0.contents,
        );
        log::info!("{}|{} message with priority of {} proxied, {} queue_requests, {} queued, {} proxy requests, {} proxied, {} in {} queue, request took {} ms",
            milliseconds_since_timestamp(server_started.0),
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
        let debug;
        if cfg!(feature = "rqueue-debug") {
            debug = json!({
                "queue_requests": queue_requests,
                "proxy_requests": proxy_requests,
                "queued": queued,
                "proxied": proxied,
                "in_queue": in_queue,
                "uptime": milliseconds_since_timestamp(server_started.0),
                "process_time": milliseconds_since_timestamp(request_started.0),
                "queue_size": format!("{}", Size::Bytes(bytes_allocated_for_queue)),
            })
        }
        else {
            debug = json!({})
        }
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
                    "debug": debug,
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

fn rocket(server_started: Duration) -> rocket::Rocket {
    rocket::ignite()
        .manage(Started(server_started))
        .attach(AdHoc::on_attach("Custom Configuration", |rocket| {
            let mut queue_config: QueueConfig = QueueConfig::default();
            queue_config.memory_limit = match rocket.config().get_int("queue_memory_limit_in_bytes") {
                Ok(n) => n as usize,
                Err(_) => DEFAULT_MAXIMUM_QUEUE_SIZE,
            };
            log::info!("Queue memory limit: {}", Size::Bytes(queue_config.memory_limit));

            queue_config.require_sha256 = match rocket.config().get_bool("require_sha256") {
                Ok(n) => n,
                Err(_) => false,
            };
            log::info!("Require sha256: {}", queue_config.require_sha256);

            queue_config.shared_secret = match rocket.config().get_str("shared_secret") {
                Ok(n) => n.to_string(),
                Err(_) => "".to_string(),
            };
            log::info!("Shared secret: {}", queue_config.shared_secret);

            if cfg!(feature = "rqueue-proxy") {
                let mut proxy_config = PROXY_CONFIG.lock().unwrap();
                proxy_config.delay = match rocket.config().get_int("proxy_delay") {
                    Ok(n) => {
                        if n > 0 {
                            n as usize
                        }
                        else {
                            DEFAULT_DELAY
                        }
                    }
                    Err(_) => DEFAULT_DELAY,
                };
                log::info!("Proxy delay: {} s", proxy_config.delay);
                proxy_config.server = match rocket.config().get_str("notification_server") {
                    Ok(n) => n.to_string(),
                    Err(_) => {
                        log::error!("Fatal error: 'notification_server' was not found in Rocket.toml.");
                        process::exit(1);
                    }
                };
                log::info!("Notification server: {}", proxy_config.server);
            }

            if cfg!(feature = "rqueue-notify") {
                let mut notify_config = NOTIFY_CONFIG.lock().unwrap();
                notify_config.delay = match rocket.config().get_int("notify_delay") {
                    Ok(n) => {
                        if n > 0 {
                            n as usize
                        }
                        else {
                            DEFAULT_DELAY
                        }
                    }
                    Err(_) => DEFAULT_DELAY,
                };
                log::info!("Notify delay: {}", notify_config.delay);
                notify_config.mail_from_name = match rocket.config().get_string("mail_from_name") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("Mail from name: {}", notify_config.mail_from_name);
                notify_config.mail_from_address = match rocket.config().get_string("mail_from_address") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("Mail from address: {}", notify_config.mail_from_address);
                notify_config.mail_to_name = match rocket.config().get_string("mail_to_name") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("Mail to name: {}", notify_config.mail_to_name);
                notify_config.mail_to_address = match rocket.config().get_string("mail_to_address") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("Mail to address: {}", notify_config.mail_to_address);
                notify_config.smtp_server = match rocket.config().get_string("smtp_server") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("SMTP server: {}", notify_config.smtp_server);
                notify_config.smtp_user = match rocket.config().get_string("smtp_user") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("SMTP user: {}", notify_config.smtp_user);
                notify_config.smtp_password = match rocket.config().get_string("smtp_password") {
                    Ok(n) => n.to_string(),
                    Err(_) => "".to_string(),
                };
                log::info!("SMTP password: {}", notify_config.smtp_password);
            }

            Ok(rocket.manage(queue_config))
        }))
        .attach(AdHoc::on_request("Time Request", |req, _| {
            req.local_cache(|| RequestTimer(time_since_epoch()));
        }))
        .register(catchers![not_found])
        .mount("/", routes![new, get])
}

fn main() {
    let server_started = time_since_epoch();

    if cfg!(feature = "rqueue-proxy") {
        // Proxy thread reads queue and pushes notifications upstream.
        thread::spawn(move || {
            // Make a copy for the proxy thread
            let proxy_started = server_started;
            proxy::proxy_loop(proxy_started);
        });
    }

    if cfg!(feature = "rqueue-notify") {
        // Notify thread reads queue and generates notifications.
        thread::spawn(move || {
            // Make a copy for the proxy thread
            //let notify_started = server_started;
            notify::notify_loop(server_started.clone());
        });
    }

    // REST server collects notifications in the queue.
    rocket(server_started).launch();
}

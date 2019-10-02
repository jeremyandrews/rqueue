use std::thread;
use std::time::Duration;
use std::sync::atomic::Ordering;
use serde_json::json;

use crate::{COUNTERS, QUEUE, PROXY_CONFIG, DEFAULT_DELAY, milliseconds_since_timestamp, MessageInternal};

use size::{Base, Size, Style};


pub fn proxy_loop(server_started: Duration) {
    let mut sleep_time = DEFAULT_DELAY;
    loop {
        log::debug!("{}|top of proxy loop", milliseconds_since_timestamp(server_started));
        thread::sleep(Duration::from_secs(sleep_time as u64));

        let queue_contents;
        // We preserve a copy of the message in case there's an error, as then we'll
        // return it to the queue.
        let mut message: MessageInternal = MessageInternal::default();
        let server;
        {
            // We don't use counters here, but we have to grab locks in order to prevent a race
            let _counters = COUNTERS.lock().unwrap();
            let mut queue = QUEUE.lock().expect("queue lock");
            queue_contents = queue.pop().map(|internal| {
                message.size_in_bytes = internal.0.size_in_bytes;
                message.contents = internal.0.contents.clone();
                message.sha256 = internal.0.sha256.clone();
                message.priority = internal.0.priority;
                message.arrived = internal.0.arrived;
                message.uuid = internal.0.uuid.clone();
                message.original_priority = internal.0.original_priority;
                message.delivery_attempts = internal.0.delivery_attempts + 1;
            });
            let proxy_config = PROXY_CONFIG.lock().unwrap();
            server = proxy_config.server.clone();
        }

        let response;
        if queue_contents != None {
            let message_json = json!({
                "contents": &message.contents.clone(),
                "priority": message.priority.clone(),
                "sha256": &message.sha256.clone(),
                "uuid": &message.uuid.clone(),
                // DEBUG
            });

            log::debug!("{}|message from queue with sha256 {}: '{}'",
                milliseconds_since_timestamp(server_started),
                &message.sha256,
                &message.contents,
            );

            let client = reqwest::Client::new();
            response = client.post(&server)
                .json(&message_json)
                .send();

            match response {
                Ok(_) => {
                    sleep_time = 0;
                    let counters = COUNTERS.lock().unwrap();
                    // A message has been sucessfully removed from the queue.
                    let proxied = counters.proxied.fetch_add(1, Ordering::Relaxed) + 1;
                    let in_queue = counters.in_queue.fetch_sub(1, Ordering::Relaxed) - 1;
                    let bytes_allocated_for_queue = counters.bytes.fetch_sub(message.size_in_bytes, Ordering::Relaxed) - message.size_in_bytes;
                    // Retreive other debug statistics
                    let queue_requests = counters.queue_requests.load(Ordering::Relaxed);
                    let queued = counters.queued.load(Ordering::Relaxed);

                    log::info!("{}|{} message with priority of {} proxied, {} queue_requests, {} queued, {} proxied, {} in {} queue",
                        milliseconds_since_timestamp(server_started),
                        Size::Bytes(message.size_in_bytes).to_string(Base::Base10, Style::Abbreviated),
                        message.priority,
                        queue_requests,
                        queued,
                        proxied,
                        in_queue,
                        Size::Bytes(bytes_allocated_for_queue).to_string(Base::Base10, Style::Abbreviated),
                    );
                }
                Err(e) => {
                    sleep_time = DEFAULT_DELAY;
                    if e.is_server_error() {
                        log::warn!("{}|proxy failure {} to '{}', upstream server error: {}",
                            milliseconds_since_timestamp(server_started),
                            &message.delivery_attempts,
                            &server,
                            e
                        );
                    }
                    else if e.is_client_error() {
                        log::warn!("{}|proxy failure {} to '{}', local configuration error: {}",
                            milliseconds_since_timestamp(server_started),
                            &message.delivery_attempts,
                            &server,
                            e
                        );
                    }
                    else if e.is_http() {
                        match e.url() {
                            None => {
                                log::warn!("{}|proxy failure {} to '{}', no url configured [{}]",
                                    milliseconds_since_timestamp(server_started),
                                    &message.delivery_attempts,
                                    &server,
                                    e
                                );
                            }
                            Some(url) => {
                                log::warn!("{}|proxy failure {} to '{}', invalid url configured [{}]",
                                    milliseconds_since_timestamp(server_started),
                                    &message.delivery_attempts,
                                    &server,
                                    url
                                );
                            }
                        }
                    }
                    else {
                        log::warn!("{}|proxy failure {} to '{}', unexpected error [{}]",
                            milliseconds_since_timestamp(server_started),
                            &message.delivery_attempts,
                            &server,
                            e
                        );
                    }
                    let priority = message.priority;
                    // We don't need counters here, but we have to grab locks in order to avoid a race
                    let _counters = COUNTERS.lock().unwrap();
                    let mut queue = QUEUE.lock().expect("queue lock");
                    queue.push(message, priority);
                }
            }
        }
        else {
            // If the queue is empty, sleep longer.
            let proxy_config = PROXY_CONFIG.lock().unwrap();
            sleep_time = proxy_config.delay;
        }
        log::debug!("{}|bottom of proxy loop", milliseconds_since_timestamp(server_started));
    }
}
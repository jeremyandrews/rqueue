use std::thread;
use std::time::Duration;
use std::sync::atomic::Ordering;

use crate::{STARTED, QUEUE, COUNTERS, milliseconds_since_timestamp};

use size::Size;


pub fn proxy_loop() {
    let mut sleep_time = 1;
    loop {
        thread::sleep(Duration::from_secs(sleep_time));
        let server_started = STARTED.lock().unwrap();
        let counters = COUNTERS.lock().unwrap();
        log::debug!("{}|top of proxy loop", milliseconds_since_timestamp(*server_started));

        let mut queue = QUEUE.lock().expect("queue lock");
        let contents = queue.pop().map(|internal| {
            // @TODO: Actually push the request upstream -- if this fails, return the item
            // to the queue.
            sleep_time = 0;
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
            log::info!("{}|{} message with priority of {} proxied, {} queue_requests, {} queued, {} proxied, {} in {} queue",
                milliseconds_since_timestamp(*server_started),
                Size::Bytes(internal.0.size_in_bytes),
                internal.0.priority,
                queue_requests,
                queued,
                proxied,
                in_queue,
                Size::Bytes(bytes_allocated_for_queue),
            );
        });
        // If the queue is empty, sleep longer.
        if contents == None {
            sleep_time = 1;
        }
        log::debug!("{}|bottom of proxy loop", milliseconds_since_timestamp(*server_started));
    }
}
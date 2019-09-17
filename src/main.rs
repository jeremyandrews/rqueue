#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use priority_queue::PriorityQueue;

use rocket::State;
use rocket_contrib::json::{Json, JsonValue};

type Priority = usize;
type MessageQueue = Mutex<PriorityQueue<String, Priority>>;
#[derive(Serialize, Deserialize)]
struct Message {
    contents: String,
}

#[post("/?<priority>", format="json", data="<message>")]
fn new(priority: Option<Priority>, message: Json<Message>, queue: State<'_, MessageQueue>) -> JsonValue {
    let prio: Priority;
    match priority {
        None => {
            prio = 10;
        }
        _ => {
            prio = priority.unwrap();
        }
    }
    let mut messagequeue = queue.lock().expect("queue lock.");
    messagequeue.push(message.0.contents, prio);
    json!({
        "status": "ok",
    })
}

#[get("/", format = "json")]
fn get(queue: State<'_, MessageQueue>) -> Option<Json<Message>> {
    let mut messagequeue = queue.lock().unwrap();
    messagequeue.pop().map(|contents| {
        Json(Message {
            contents: contents.0.clone(),
        })
    })
}

#[catch(404)]
fn not_found() -> JsonValue {
    json!({
        "status": "error",
        "reason": "Resource was not found.",
    })
}

fn rocket() -> rocket::Rocket {
    rocket::ignite()
        .mount("/", routes![new, get])
        .register(catchers![not_found])
        .manage(Mutex::new(PriorityQueue::<String, Priority>::new()))
}

fn main() {
    rocket().launch();
}

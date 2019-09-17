#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;

#[cfg(test)] mod tests;

use std::sync::Mutex;
use std::collections::VecDeque;

use rocket::State;
use rocket_contrib::json::{Json, JsonValue};

type MessageQueue = Mutex<VecDeque<String>>;
#[derive(Serialize, Deserialize)]
struct Message {
    contents: String,
}

#[post("/", format="json", data="<message>")]
fn new(message: Json<Message>, queue: State<'_, MessageQueue>) -> JsonValue {
    let mut messagequeue = queue.lock().expect("queue lock.");
    messagequeue.push_back(message.0.contents);
    json!({
        "status": "ok",
    })
}

#[get("/", format = "json")]
fn get(queue: State<'_, MessageQueue>) -> Option<Json<Message>> {
    let mut messagequeue = queue.lock().unwrap();
    messagequeue.pop_front().map(|contents| {
        Json(Message {
            contents: contents.clone(),
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
        .mount("/message", routes![new, get])
        .register(catchers![not_found])
        .manage(Mutex::new(VecDeque::<String>::new()))
}

fn main() {
    rocket().launch();
}

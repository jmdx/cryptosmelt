use app::App;
use std::sync::Arc;
use std::thread;
use rocket;
use rocket::*;
use rocket::http::*;
use rocket_contrib::Json;
use serde_json::*;

#[get("/poolstats")]
fn poolstats(app: State<Arc<App>>) -> Json<Value> {
  let hashrates = app.db.get_hashrates();
  Json(json!({
    "hashrates": hashrates,
  }))
}

#[get("/minerstats/<address>")]
fn minerstats(app: State<Arc<App>>, address: &RawStr) -> Json<Value> {
  let address = address.as_str();
  if !app.address_pattern.is_match(address) {
    let no_data: Vec<String> = Vec::new();
    Json(json!({
      "hashrates": no_data
    }))
  }
  else {
    let no_data: Vec<String> = Vec::new();
    // TODO reimplement this in postgres
    Json(json!({
      "hashrates": no_data,
    }))
  }
}

pub fn init(app: Arc<App>) {
  thread::spawn(move || {
    rocket::ignite()
      .manage(app)
      .mount("/", routes![poolstats, minerstats]).launch();
  });
}

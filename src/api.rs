use app::App;
use std::sync::Arc;
use std::thread;
use rocket;
use rocket::*;
use rocket::http::*;
use rocket_contrib::Json;
use serde_json::*;

const INTERVAL_SECS: u32 = 5 * 60;

#[get("/poolstats")]
fn poolstats(app: State<Arc<App>>) -> Json<Value> {
  let hashrates = app.db.query(
    &format!(
      // The influx client we use doesn't seem to have support for the groupings influx returns,
      // so the nested select is needed to conver the groups into a flat form.
      // Also on some systems, influx seems to have a bug where it ignores sum(...), so we have to
      // improvise with mean * count.
      "SELECT * FROM (SELECT mean(value) * count(value) / {} FROM valid_share WHERE time > now() - 1d \
       GROUP BY time({}s), alias fill(none))",
      INTERVAL_SECS,
      INTERVAL_SECS,
    )
  );
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
    let hashrates = app.db.query(
      &format!(
        "SELECT * FROM (SELECT mean(value) * count(value) / {} FROM valid_share WHERE time > now() - 1d \
         AND address='{}'\
         GROUP BY time({}s), alias fill(none))",
        INTERVAL_SECS,
        // The check against app.address_pattern verifies that the address is alphanumeric, so we
        // don't have to worry about injection.
        address,
        INTERVAL_SECS,
      )
    );
    Json(json!({
      "hashrates": hashrates,
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

use crate::models::config::{Config, RawConfig};
use crate::{run_query, Query};
use actix_cors::Cors;
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder, Scope};
use sqlx::SqlitePool;
use tracing_actix_web::TracingLogger;

use crate::db::connect;

#[derive(Debug, Clone)]
pub struct AppState {
    pub conn: SqlitePool,
}

pub async fn run_server() -> std::io::Result<()> {
    println!("Starting API...");
    let conn = connect().await;
    let state = AppState { conn };
    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);
        App::new()
            .wrap(cors)
            .wrap(TracingLogger::default())
            .app_data(web::Data::new(state.clone()))
            .service(routes())
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}

pub fn routes() -> Scope {
    web::scope("")
        .service(list_configs)
        .service(query_map)
        .service(query_list)
    //.service(data_source::routes())
    //.service(meta::routes())
    //.service(user::routes())
    //.service(export::routes())
    //.service(event::routes())
    //.service(backup::routes())
    //.service(key::routes())
    //.service(timer::routes())
    //.service(ui_utils::routes())
}

#[get("/")]
async fn list_configs(data: web::Data<AppState>) -> impl Responder {
    let configs = sqlx::query_as!(
        RawConfig,
        r#"
       SELECT * FROM "config"
    "#
    )
    .fetch_all(&data.conn)
    .await
    .expect("Failed to retrieve configs")
    .into_iter()
    .map(Config::from)
    .collect::<Vec<_>>();
    HttpResponse::Ok().json(configs)
}

#[get("/query")]
async fn query_map(data: web::Data<AppState>, query: web::Query<Query>) -> impl Responder {
    let r = run_query(&data.conn, query.into_inner())
        .await
        .expect("Failed to run query");
    HttpResponse::Ok().json(r)
}

#[get("/query_list")]
async fn query_list(data: web::Data<AppState>, query: web::Query<Query>) -> impl Responder {
    let r = run_query(&data.conn, query.into_inner())
        .await
        .expect("Failed to run query");
    let mut values: Vec<_> = r.values().flatten().collect();
    values.sort_by_key(|r| r.fetched_at);
    HttpResponse::Ok().json(values)
}

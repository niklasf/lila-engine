use std::net::SocketAddr;

use axum::{
    extract::{FromRef, Json, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use axum_extra::routing::{RouterExt, TypedPath};
use clap::Parser;
use serde::Deserialize;
use thiserror::Error;
use tokio::{
    sync::mpsc::{channel, Sender},
    task,
};

use crate::{
    api::{AcquireRequest, AnalyseRequest, EngineId, ProviderSelector},
    hub::{Hub, IsValid},
    ongoing::Ongoing,
    repo::Repo,
};

mod api;
mod hub;
mod ongoing;
mod repo;

#[derive(Parser)]
struct Opt {
    /// Binding address.
    #[clap(long, default_value = "127.0.0.1:9666")]
    pub bind: SocketAddr,
    /// Database.
    #[clap(long, default_value = "mongodb://localhost")]
    pub mongodb: String,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct WorkId(String);

struct Work {
    tx: Sender<()>,
}

impl IsValid for Work {
    fn is_valid(&self) -> bool {
        !self.tx.is_closed()
    }
}

struct AppState {
    repo: &'static Repo,
    hub: &'static Hub<ProviderSelector, Work>,
    ongoing: &'static Ongoing<WorkId, Work>,
}

impl FromRef<AppState> for &'static Repo {
    fn from_ref(state: &AppState) -> &'static Repo {
        state.repo
    }
}

impl FromRef<AppState> for &'static Hub<ProviderSelector, Work> {
    fn from_ref(state: &AppState) -> &'static Hub<ProviderSelector, Work> {
        state.hub
    }
}

impl FromRef<AppState> for &'static Ongoing<WorkId, Work> {
    fn from_ref(state: &AppState) -> &'static Ongoing<WorkId, Work> {
        state.ongoing
    }
}

#[derive(Error, Debug, Clone)]
enum Error {
    #[error("mongodb error: {0}")]
    MongoDb(#[from] mongodb::error::Error),
    #[error("engine not found or invalid clientSecret")]
    EngineNotFound,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Error::MongoDb(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::EngineNotFound => StatusCode::NOT_FOUND,
        };
        (status, self.to_string()).into_response()
    }
}

#[tokio::main]
async fn main() {
    let opt = Opt::parse();

    let state = AppState {
        repo: Box::leak(Box::new(Repo::new(&opt.mongodb).await)),
        hub: Box::leak(Box::new(Hub::new())),
        ongoing: Box::leak(Box::new(Ongoing::new())),
    };

    task::spawn(state.hub.garbage_collect());
    task::spawn(state.ongoing.garbage_collect());

    let app = Router::with_state(state)
        .typed_post(analyse)
        .route("/api/external-engine/work", post(acquire))
        .route("/api/external-engine/submit", post(submit));

    axum::Server::bind(&opt.bind)
        .serve(app.into_make_service())
        .await
        .expect("bind");
}

#[derive(TypedPath, Deserialize)]
#[typed_path("/api/external-engine/:id/analyse")]
struct AnalysePath {
    id: EngineId,
}

#[axum_macros::debug_handler(state = AppState)]
async fn analyse(
    AnalysePath { id }: AnalysePath,
    State(hub): State<&'static Hub<ProviderSelector, Work>>,
    State(repo): State<&'static Repo>,
    Json(req): Json<AnalyseRequest>,
) -> Result<(), Error> {
    let engine = repo
        .find(id, req.client_secret)
        .await?
        .ok_or(Error::EngineNotFound)?;
    let (tx, rx) = channel(4);
    hub.submit(engine.provider_secret.selector(), Work { tx });
    Ok(())
}

#[axum_macros::debug_handler(state = AppState)]
async fn acquire(
    State(hub): State<&'static Hub<ProviderSelector, Work>>,
    State(ongoing): State<&'static Ongoing<WorkId, Work>>,
    Json(req): Json<AcquireRequest>,
) {
    let selector = req.provider_secret.selector();
    let work = hub.acquire(selector).await;
    ongoing.add(todo!(), work);
}

#[axum_macros::debug_handler(state = AppState)]
async fn submit(State(ongoing): State<&'static Ongoing<WorkId, Work>>) {
    let work = ongoing.remove(todo!());
}

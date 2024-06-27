use actix_web::{web, App, HttpServer, middleware};
use std::sync::Arc;
use tokio::sync::Mutex;
use libp2p;

use crate::{network::NetworkClient, openfaas::OpenFaasClient};
use crate::http_server::routes::routes;

pub(crate) struct AppState {
    pub(crate) nc: Arc<Mutex<NetworkClient>>,
    pub(crate) ofc: Arc<Mutex<OpenFaasClient>>,
    pub(crate) peer_id: libp2p::PeerId,
}
impl AppState {
    pub(crate) fn new(
        nc: Arc<Mutex<NetworkClient>>,
        ofc: Arc<Mutex<OpenFaasClient>>,
        peer_id: libp2p::PeerId,
    ) -> Self {
        Self { nc, ofc, peer_id }
    }
}

pub async fn run_http_server(app_state: AppState, port: u16) -> std::io::Result<()> {
    let data = web::Data::new(app_state);

    let server = HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .wrap(middleware::Logger::default())
            .configure(routes)
    });

    server.bind(("127.0.0.1", port))?.run().await
}
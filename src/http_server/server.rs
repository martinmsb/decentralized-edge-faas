use actix_web::{web, App, HttpServer, middleware};
use std::sync::Arc;
use tokio::sync::Mutex;
use libp2p;

use crate::{network::NetworkClient, openfaas::OpenFaasClient, data_structures::RequestsInProgress};
use crate::http_server::routes::routes;

pub(crate) struct AppState {
    pub(crate) nc: Arc<NetworkClient>,
    pub(crate) ofc: Arc<OpenFaasClient>,
    pub(crate) peer_id: libp2p::PeerId,
    pub(crate) rp: Arc<Mutex<RequestsInProgress>>,
}
impl AppState {
    pub(crate) fn new(
        nc: Arc<NetworkClient>,
        ofc: Arc<OpenFaasClient>,
        peer_id: libp2p::PeerId,
        rp: Arc<Mutex<RequestsInProgress>>,
    ) -> Self {
        Self { nc, ofc, peer_id, rp }
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

    server.bind(("0.0.0.0", port))?.run().await
}
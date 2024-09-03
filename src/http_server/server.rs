use actix_web::{web, App, HttpServer, middleware};
use std::sync::Arc;

use crate::functions_service::FunctionsService;
use crate::http_server::routes::routes;

pub(crate) struct AppState {
    pub(crate) fs: Arc<FunctionsService>,
}
impl AppState {
    pub(crate) fn new(
        fs: Arc<FunctionsService>,
    ) -> Self {
        Self { fs }
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